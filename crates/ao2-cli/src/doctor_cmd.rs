use super::{json_string, read_json_file, runtime_target_label, terminate_workbench_child};
use crate::cli_util::binary_name_for_target;
use crate::install_paths::{
    binary_on_path, command_exists, default_install_dir, install_verification_evidence_path,
    is_binary_on_path,
};
use crate::release_provenance::verify_release_provenance_signature;
use anyhow::Result;
use ao2_adapters::{doctor_provider, parse_provider};
use ao2_runtime::{
    expected_doctor_release_assets, is_hosted_release_directory, verify_hosted_release_directory,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Duration;
pub(crate) fn doctor(
    json: bool,
    install_dir: Option<PathBuf>,
    provenance_dir: PathBuf,
    release: Option<String>,
    release_asset_dir: Option<PathBuf>,
    release_repo: String,
) -> Result<()> {
    let report = doctor_report_json(
        install_dir,
        provenance_dir,
        release,
        release_asset_dir,
        release_repo,
    )?;
    let status = json_string(&report, "status");
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("status={status}");
        println!("details={}", serde_json::to_string_pretty(&report)?);
    }
    Ok(())
}
pub(crate) fn doctor_report_json(
    install_dir: Option<PathBuf>,
    provenance_dir: PathBuf,
    release: Option<String>,
    release_asset_dir: Option<PathBuf>,
    release_repo: String,
) -> Result<serde_json::Value> {
    let target = runtime_target_label();
    let binary_name = binary_name_for_target(&target);
    let install_dir = install_dir.unwrap_or_else(|| {
        let default = default_install_dir();
        binary_on_path(binary_name)
            .and_then(|binary| binary.parent().map(Path::to_path_buf))
            .unwrap_or(default)
    });
    let installed_binary = install_dir.join(binary_name);
    let installed = installed_binary.is_file();
    let on_path = installed && is_binary_on_path(binary_name, &installed_binary);
    let install_verification_evidence =
        doctor_install_verification_evidence_json(&installed_binary);
    let release_public_key = provenance_dir.join("ao2-release-signing-public.pem");
    let provenance_json = provenance_dir.join("ao2-release-provenance.json");
    let provenance_signature = provenance_dir.join("ao2-release-provenance.json.sig");
    let provenance_verified = verify_release_provenance_signature(
        &provenance_json,
        &provenance_signature,
        &release_public_key,
    );
    let mut release_report = serde_json::json!({
        "checked": false,
        "provenance_dir": provenance_dir,
        "public_key": release_public_key,
        "provenance_json": provenance_json,
        "provenance_signature": provenance_signature,
        "provenance_verified": provenance_verified,
    });
    if let Some(release_tag) = release {
        release_report = doctor_release_report_json(
            release_tag,
            release_repo,
            release_asset_dir,
            &provenance_json,
            provenance_verified,
        );
    }
    let scripted = doctor_provider(parse_provider("scripted")?)?;
    let providers = serde_json::json!({
        "scripted": scripted,
    });
    let dependencies = serde_json::json!({
        "native_crypto": true,
        "curl": command_exists("curl"),
        "tar": command_exists("tar"),
        "gh": command_exists("gh"),
    });
    let dependencies_ok = ["native_crypto", "curl", "tar"]
        .into_iter()
        .all(|name| dependencies[name].as_bool().unwrap_or(false));
    let release_checked = release_report["checked"].as_bool().unwrap_or(false);
    let release_provenance_verified = release_report["provenance_verified"]
        .as_bool()
        .unwrap_or(false);
    let release_ok = !release_checked
        || (release_report["assets_available"]
            .as_bool()
            .unwrap_or(false)
            && release_provenance_verified
            && release_report["provenance_tag_matches"]
                .as_bool()
                .unwrap_or(false));
    let provenance_ok = if release_checked {
        release_provenance_verified
    } else {
        provenance_verified
            || doctor_install_verification_evidence_is_valid(
                &install_verification_evidence,
                &target,
            )
    };
    let status = if installed
        && on_path
        && provenance_ok
        && scripted.available
        && dependencies_ok
        && release_ok
    {
        "ok"
    } else {
        "attention"
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.doctor.v1",
        "status": status,
        "version": env!("CARGO_PKG_VERSION"),
        "target": target,
        "install": {
            "install_dir": install_dir,
            "binary": installed_binary,
            "installed": installed,
            "on_path": on_path,
            "verification_evidence": install_verification_evidence,
        },
        "release": release_report,
        "providers": providers,
        "dependencies": dependencies,
    }))
}
fn doctor_install_verification_evidence_is_valid(
    evidence: &serde_json::Value,
    target: &str,
) -> bool {
    evidence["present"].as_bool() == Some(true)
        && evidence["schema_version"].as_str() == Some("ao2.install-verification-evidence.v1")
        && evidence["status"].as_str() == Some("verified")
        && evidence["install_status"].as_str() == Some("installed")
        && evidence["version"].as_str() == Some(env!("CARGO_PKG_VERSION"))
        && evidence["target"].as_str() == Some(target)
        && evidence["offline_verification"]["schema_version"].as_str()
            == Some("ao2.release-archive-offline-verification.v1")
        && evidence["offline_verification"]["status"].as_str() == Some("verified")
        && evidence["offline_verification"]["checksum_coverage_verified"].as_bool() == Some(true)
        && evidence["binary_path_matches"].as_bool() != Some(false)
}
fn doctor_install_verification_evidence_json(installed_binary: &Path) -> serde_json::Value {
    let evidence_path = install_verification_evidence_path(installed_binary);
    if !evidence_path.is_file() {
        return serde_json::json!({
            "present": false,
            "status": "missing",
            "path": evidence_path,
        });
    }
    match read_json_file::<serde_json::Value>(&evidence_path) {
        Ok(evidence) => {
            let binary_path_matches = evidence["installed_binary"]
                .as_str()
                .map(|recorded| {
                    fs::canonicalize(recorded)
                        .and_then(|recorded| {
                            fs::canonicalize(installed_binary)
                                .map(|installed| installed == recorded)
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(true);
            serde_json::json!({
                "present": true,
                "path": evidence_path,
                "schema_version": evidence["schema_version"],
                "status": evidence["status"],
                "install_status": evidence["install_status"],
                "version": evidence["version"],
                "target": evidence["target"],
                "installed_binary": evidence["installed_binary"],
                "binary_path_matches": binary_path_matches,
                "signature_verified": evidence["signature_verified"],
                "offline_verification": evidence["offline_verification"],
            })
        }
        Err(error) => serde_json::json!({
            "present": true,
            "status": "invalid",
            "path": evidence_path,
            "error": error.to_string(),
        }),
    }
}
fn doctor_release_report_json(
    release_tag: String,
    release_repo: String,
    release_asset_dir: Option<PathBuf>,
    provenance_json: &Path,
    provenance_verified: bool,
) -> serde_json::Value {
    let version = release_tag.strip_prefix('v').unwrap_or(&release_tag);
    let hosted_directory = release_asset_dir
        .as_deref()
        .is_some_and(is_hosted_release_directory);
    let expected_assets = expected_doctor_release_assets(release_asset_dir.as_deref(), version);
    let provenance_tag = fs::read_to_string(provenance_json)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .map(|json| json_string(&json, "release_tag"))
        .unwrap_or_default();
    let provenance_tag_matches = provenance_tag == release_tag;
    let mut report = serde_json::json!({
        "checked": true,
        "release_tag": release_tag,
        "release_repo": release_repo,
        "expected_assets": expected_assets,
        "provenance_verified": provenance_verified,
        "provenance_tag": provenance_tag,
        "provenance_tag_matches": provenance_tag_matches,
    });
    if let Some(asset_dir) = release_asset_dir {
        let present_assets = expected_assets
            .iter()
            .filter(|asset| asset_dir.join(asset).is_file())
            .cloned()
            .collect::<Vec<_>>();
        let missing_assets = expected_assets
            .iter()
            .filter(|asset| !asset_dir.join(asset).is_file())
            .cloned()
            .collect::<Vec<_>>();
        report["asset_source"] = serde_json::json!("directory");
        report["asset_dir"] = serde_json::json!(asset_dir);
        report["asset_count"] = serde_json::json!(present_assets.len());
        report["assets_available"] = serde_json::json!(missing_assets.is_empty());
        report["present_assets"] = serde_json::json!(present_assets);
        report["missing_assets"] = serde_json::json!(missing_assets);
        report["rollback"] = release_rollback_summary_json(&asset_dir);
        if hosted_directory {
            let verification = verify_hosted_release_directory(&asset_dir, version, &release_tag);
            report["hosted_contract"] = verification.report;
            report["provenance_verified"] = serde_json::json!(verification.verified);
            report["provenance_tag"] = serde_json::json!(release_tag);
            report["provenance_tag_matches"] = serde_json::json!(verification.tag_matches);
        }
        return report;
    }
    if resolve_gh_command().is_none() {
        report["asset_source"] = serde_json::json!("github");
        report["assets_available"] = serde_json::json!(false);
        report["asset_count"] = serde_json::json!(0);
        report["error"] = serde_json::json!("gh_not_available");
        return report;
    }
    let Some(tag) = report["release_tag"].as_str() else {
        report["assets_available"] = serde_json::json!(false);
        return report;
    };
    let Some(repo) = report["release_repo"].as_str() else {
        report["assets_available"] = serde_json::json!(false);
        return report;
    };
    let output = run_gh_release_view_for_doctor(tag, repo);
    match output {
        Ok(output) if output.status.success() => {
            let json = serde_json::from_slice::<serde_json::Value>(&output.stdout)
                .unwrap_or_else(|_| serde_json::json!({}));
            let names = json["assets"]
                .as_array()
                .map(|assets| {
                    assets
                        .iter()
                        .filter_map(|asset| asset["name"].as_str().map(str::to_string))
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            let missing_assets = expected_assets
                .iter()
                .filter(|asset| !names.contains(*asset))
                .cloned()
                .collect::<Vec<_>>();
            report["asset_source"] = serde_json::json!("github");
            report["asset_count"] = serde_json::json!(names.len());
            report["assets_available"] = serde_json::json!(missing_assets.is_empty());
            report["present_assets"] = serde_json::json!(names);
            report["missing_assets"] = serde_json::json!(missing_assets);
            report["is_draft"] = json["isDraft"].clone();
            report["is_prerelease"] = json["isPrerelease"].clone();
        }
        Ok(output) => {
            report["asset_source"] = serde_json::json!("github");
            report["assets_available"] = serde_json::json!(false);
            report["asset_count"] = serde_json::json!(0);
            report["error"] = serde_json::json!(String::from_utf8_lossy(&output.stderr).trim());
        }
        Err(error) => {
            report["asset_source"] = serde_json::json!("github");
            report["assets_available"] = serde_json::json!(false);
            report["asset_count"] = serde_json::json!(0);
            report["error"] = serde_json::json!(error.to_string());
        }
    }
    report
}
fn run_gh_release_view_for_doctor(tag: &str, repo: &str) -> std::io::Result<std::process::Output> {
    let Some((gh, shell_script)) = resolve_gh_command() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "gh_not_available",
        ));
    };
    let mut command = if shell_script {
        let mut command = ProcessCommand::new("cmd.exe");
        command.args(["/d", "/s", "/c"]).arg(gh);
        command
    } else {
        ProcessCommand::new(gh)
    };
    let mut child = command
        .args([
            "release",
            "view",
            tag,
            "--repo",
            repo,
            "--json",
            "assets,isDraft,isPrerelease",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let timeout = std::env::var("AO2_DOCTOR_GH_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(30));
    let started = std::time::Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if started.elapsed() >= timeout {
            terminate_workbench_child(&mut child);
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "gh_timed_out",
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}
fn resolve_gh_command() -> Option<(PathBuf, bool)> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        if cfg!(windows) {
            for (name, shell_script) in [
                ("gh.exe", false),
                ("gh.cmd", true),
                ("gh.bat", true),
                ("gh.com", false),
                ("gh", false),
            ] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some((candidate, shell_script));
                }
            }
        } else {
            let candidate = dir.join("gh");
            if candidate.is_file() {
                return Some((candidate, false));
            }
        }
    }
    None
}
fn release_rollback_summary_json(asset_dir: &Path) -> serde_json::Value {
    let summary = asset_dir.join("release-rollback-summary.json");
    if !summary.is_file() {
        return serde_json::json!({
            "checked": false,
            "status": "missing",
            "summary_json": summary,
        });
    }
    match fs::read_to_string(&summary)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
    {
        Some(mut json) => {
            json["checked"] = serde_json::json!(true);
            json["summary_json"] = serde_json::json!(summary);
            json
        }
        None => serde_json::json!({
            "checked": false,
            "status": "invalid",
            "summary_json": summary,
        }),
    }
}
