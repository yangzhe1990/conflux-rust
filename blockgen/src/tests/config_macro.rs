macro_rules! if_option {
	(Option<$type:ty>, THEN {$($then:tt)*} ELSE {$($otherwise:tt)*}) => (
		$($then)*
	);
	($type:ty, THEN {$($then:tt)*} ELSE {$($otherwise:tt)*}) => (
		$($otherwise)*
	);
}

macro_rules! underscore_to_hyphen {
    ($e:expr) => {
        str::replace($e, "_", "-")
    };
}

#[macro_export]
macro_rules! build_config{
    (
        {
            $(($name:ident, ($($type:tt)+), $default:expr))*
        }
        {
            $(($c_name:ident, ($($c_type:tt)+), $c_default:expr, $converter:expr))*
        }
    ) => {
        use tests::cache_config::CacheConfig;
        use tests::clap;
        use core::db::NUM_COLUMNS;
        use tests::db;
        use tests::kvdb_rocksdb::DatabaseConfig;
        use log::LevelFilter;
        use tests::network::{node_table::validate_node_url, ErrorKind, NetworkConfiguration};
        use std::{
            fs::{self, File},
            io::prelude::*,
            net::ToSocketAddrs,
            path::Path,
            str::FromStr,
            time::Duration,
        };
        use tests::toml;

        #[derive(Debug, PartialEq, Clone)]
        pub struct RawConfiguration {
            $(pub $name: $($type)+,)*
            $(pub $c_name: $($c_type)+,)*
        }

        impl Default for RawConfiguration {
            fn default() -> Self {
                RawConfiguration {
                    $($name: $default,)*
                    $($c_name: $c_default,)*
                }
            }
        }
        impl RawConfiguration {
            // First parse arguments from config file,
            // and then parse them from commandline.
            // Replace the ones from config file with the ones
            // from commandline if duplicates.
            pub fn parse(matches: &clap::ArgMatches) -> Result<RawConfiguration, String> {
                let mut config = RawConfiguration::default();

                let config_filename = matches.value_of("config");
                if config_filename.is_some() {
                    let mut config_file = File::open(config_filename.unwrap())
                        .expect("Config file not found");

                    let mut config_str = String::new();
                    config_file
                        .read_to_string(&mut config_str)
                        .expect("Error reading config file");

                    let config_value = config_str.parse::<toml::Value>().unwrap();
                    $(
                        if let Some(value) = config_value.get(underscore_to_hyphen!(stringify!($name))) {
                            config.$name = if_option!(
                                $($type)+,
                                THEN{ Some(value.clone().try_into().map_err(|_| concat!("Invalid ", stringify!($name)).to_owned())?) }
                                ELSE{ value.clone().try_into().map_err(|_| concat!("Invalid ", stringify!($name)).to_owned())? }
                            );
                        }
                    )*
                    $(
                        if let Some(value) = config_value.get(underscore_to_hyphen!(stringify!($c_name))) {
                            config.$c_name = if_option!(
                                $($c_type)+,
                                THEN{ Some($converter(value.as_str().unwrap())?) }
                                ELSE{ $converter(value.as_str().unwrap())? }
                            )
                        }
                    )*
                }

                $(
                    if let Some(value) = matches.value_of(underscore_to_hyphen!(stringify!($name))) {
                        config.$name = if_option!(
                                $($type)+,
                                THEN{ Some(value.parse().map_err(|_| concat!("Invalid ", stringify!($name)).to_owned())?) }
                                ELSE{ value.parse().map_err(|_| concat!("Invalid ", stringify!($name)).to_owned())? }
                            );
                    }
                )*
                $(
                    if let Some(value) = matches.value_of(underscore_to_hyphen!(stringify!($c_name))) {
                    config.$c_name = if_option!(
                                $($c_type)+,
                                THEN{ Some($converter(value)?) }
                                ELSE{ $converter(value)? }
                            )
                    }
                )*
                Ok(config)
            }
        }
    }
}
