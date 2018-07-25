use account::{AccountCmd, ListAccounts, NewAccount};
use cli::{Args, ArgsError};
use run::RunCmd;

#[derive(Debug, PartialEq)]
pub enum Cmd {
    Run(RunCmd),
    Account(AccountCmd),
    Version,
}

#[derive(Debug, PartialEq)]
pub struct Configuration {
    pub args: Args,
}

impl Configuration {
    pub fn parse_cli<S: AsRef<str>>(command: &[S]) -> Result<Self, ArgsError> {
        let config = Configuration {
            args: Args::parse(command)?,
        };

        Ok(config)
    }

    pub fn into_command(&self) -> Cmd {
        let cmd = if self.args.flag_version {
            Cmd::Version
        } else if self.args.cmd_account {
            let account_cmd = if self.args.cmd_account_new {
                AccountCmd::New(NewAccount)
            } else if self.args.cmd_account_list {
                AccountCmd::List(ListAccounts)
            } else {
                unreachable!();
            };
            Cmd::Account(account_cmd)
        } else {
            let run_cmd = RunCmd {
                dirs: Default::default(),
                tcp_conf: Default::default(),
            };
            Cmd::Run(run_cmd)
        };

        cmd
    }
}
