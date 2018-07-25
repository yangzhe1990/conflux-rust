use clap;

#[derive(Debug, PartialEq)]
pub struct Configuration {
    pub port: u16,
}

impl Configuration {
    pub fn parse(matches: &clap::ArgMatches) -> Result<Configuration, String> {
        let port = match matches.value_of("port") {
            Some(port) => port.parse().map_err(|_| "Invalid port".to_owned())?,
            None => 32323,
        };

        Ok(Configuration { port: port })
    }
}
