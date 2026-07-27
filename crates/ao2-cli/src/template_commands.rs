use crate::cli::TemplateCommand;
use anyhow::Result;
pub(crate) struct TemplateSpec {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) content: &'static str,
}

#[rustfmt::skip]
pub(crate) const TASK_TEMPLATES: &[TemplateSpec] = &[
    TemplateSpec { name: "bug-fix", description: "Minimal bug fix with regression test and replayable evidence.", content: include_str!("../../../examples/task-templates/bug-fix.yaml") },
    TemplateSpec { name: "small-refactor", description: "Behavior-preserving refactor with verifier and evidence gates.", content: include_str!("../../../examples/task-templates/small-refactor.yaml") },
    TemplateSpec { name: "dependency-upgrade", description: "Single dependency upgrade with compatibility checks.", content: include_str!("../../../examples/task-templates/dependency-upgrade.yaml") },
    TemplateSpec { name: "test-generation", description: "High-value tests for existing behavior.", content: include_str!("../../../examples/task-templates/test-generation.yaml") },
    TemplateSpec { name: "rust-cargo-bug-fix", description: "Rust crate bug fix with cargo test verifier evidence.", content: include_str!("../../../examples/task-templates/rust-cargo-bug-fix.yaml") },
];

pub(crate) fn template(command: TemplateCommand) -> Result<()> {
    match command {
        TemplateCommand::List => {
            for template in TASK_TEMPLATES {
                println!("{}\t{}", template.name, template.description);
            }
            Ok(())
        }
        TemplateCommand::Show { name } => {
            let Some(template) = TASK_TEMPLATES.iter().find(|template| template.name == name)
            else {
                anyhow::bail!("unknown template: {name}");
            };
            print!("{}", template.content);
            Ok(())
        }
    }
}
