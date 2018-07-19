use cli::{Args, ArgsError};

#[derive(Debug, PartialEq)]
pub struct Configuration {
    pub args: Args,
}

impl Configuration {
    pub fn parse<S: AsRef<str>>(command: &[S]) -> Result<Self, ArgsError> {
        let args = Args::parse(command)?;

        let config = Configuration { args: args };

        Ok(config)
    }
}
