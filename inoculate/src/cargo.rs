use crate::Options;
use std::process::Command;

impl Options {
    pub fn cargo_cmd(&self, cmd: &str) -> Command {
        let mut cargo = Command::new(self.cargo_path.as_os_str());
        cargo
            .arg(cmd)
            // propagate our color mode configuration
            .env("CARGO_TERM_COLOR", self.output.color.as_str());
        cargo
    }
}
