use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(super) struct WorkbenchOperator {
    pub(super) id: String,
    pub(super) role: WorkbenchOperatorRole,
    pub(super) token: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkbenchOperatorRole {
    Viewer,
    Operator,
    Admin,
}

impl WorkbenchOperatorRole {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            WorkbenchOperatorRole::Viewer => "viewer",
            WorkbenchOperatorRole::Operator => "operator",
            WorkbenchOperatorRole::Admin => "admin",
        }
    }

    fn level(self) -> u8 {
        match self {
            WorkbenchOperatorRole::Viewer => 1,
            WorkbenchOperatorRole::Operator => 2,
            WorkbenchOperatorRole::Admin => 3,
        }
    }

    pub(super) fn can(self, required: WorkbenchOperatorRole) -> bool {
        self.level() >= required.level()
    }
}

#[derive(Clone)]
pub(crate) struct WorkbenchSupportSigning {
    pub(crate) key_path: PathBuf,
    pub(crate) signer_id: String,
}
