/// usage:
/// ```
/// build_config! {
///     {
///         (name, (type), default_value)
///         ...
///     }
///     {
///         (name, (type), default_value, converter)
///     }
/// }
/// ```
/// `converter` is a function used to convert a provided String to `Result<type,
/// String>`. For each entry, fieid `name` of type `type` will be created in
/// `RawConfiguration`, and it will be assigned to the value passed through
/// commandline argument or configuration file. Commandline argument will
/// override the configuration file if the parameter is given in both.
build_config! {
    {
        (port, (Option<u16>), None)
        (udp_port, (Option<u16>), None)
        (jsonrpc_tcp_port, (Option<u16>), None)
        (jsonrpc_http_port, (Option<u16>), None)
        (log_conf, (Option<String>), None)
        (log_file, (Option<String>), None)
        (bootnodes, (Option<String>), None)
        (netconf_dir, (Option<String>), None)
        (public_address, (Option<String>), None)
        (ledger_cache_size, (Option<usize>), None)
        (enable_discovery, (bool), false)
        (node_table_timeout, (Option<u64>), None)
        (node_table_promotion_timeout, (Option<u64>), None)
        (test_mode, (bool), false)
        (db_cache_size, (Option<usize>), None)
        (db_compaction_profile, (Option<String>), None)
        (db_dir, (Option<String>), Some("./db".to_string()))
    }
    {
        (
            log_level, (LevelFilter), LevelFilter::Info, |l| {
                match l {
                    "off" => Ok(LevelFilter::Off),
                    "error" => Ok(LevelFilter::Error),
                    "warn" => Ok(LevelFilter::Warn),
                    "info" => Ok(LevelFilter::Info),
                    "debug" => Ok(LevelFilter::Debug),
                    "trace" => Ok(LevelFilter::Trace),
                    _ => Err("Invalid log_level".to_owned()),
                }
            }
        )
    }
}

pub struct Configuration {
    pub raw_conf: RawConfiguration,
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration {
            raw_conf: Default::default(),
        }
    }
}

impl Configuration {
    pub fn parse(matches: &clap::ArgMatches) -> Result<Configuration, String> {
        let mut config = Configuration::default();
        config.raw_conf = RawConfiguration::parse(matches)?;
        Ok(config)
    }

    pub fn net_config(&self) -> NetworkConfiguration {
        let mut network_config = match self.raw_conf.port {
            Some(port) => NetworkConfiguration::new_with_port(port),
            None => NetworkConfiguration::default(),
        };

        network_config.discovery_enabled = self.raw_conf.enable_discovery;
        network_config.boot_nodes = to_bootnodes(&self.raw_conf.bootnodes)
            .expect("Error parsing bootnodes!");
        if self.raw_conf.netconf_dir.is_some() {
            network_config.config_path = self.raw_conf.netconf_dir.clone();
        }
        if let Some(addr) = self.raw_conf.public_address.clone() {
            network_config.public_address = match addr
                .to_socket_addrs()
                .map(|mut i| i.next())
            {
                Ok(sock_addr) => sock_addr,
                Err(_e) => {
                    debug!(target: "network", "public_address in config is invalid");
                    None
                }
            };
        }
        if let Some(nt_timeout) = self.raw_conf.node_table_timeout {
            network_config.node_table_timeout = Duration::from_secs(nt_timeout);
        }
        if let Some(nt_promotion_timeout) =
            self.raw_conf.node_table_promotion_timeout
        {
            network_config.connection_lifetime_for_promotion =
                Duration::from_secs(nt_promotion_timeout);
        }
        network_config.test_mode = self.raw_conf.test_mode;
        network_config
    }

    pub fn cache_config(&self) -> CacheConfig {
        let mut cache_config = CacheConfig::default();

        if let Some(db_cache_size) = self.raw_conf.db_cache_size {
            cache_config.db = db_cache_size;
        }
        if let Some(ledger_cache_size) = self.raw_conf.ledger_cache_size {
            cache_config.ledger = ledger_cache_size;
        }
        cache_config
    }

    pub fn db_config(&self) -> DatabaseConfig {
        let db_dir = self.raw_conf.db_dir.as_ref().unwrap();
        if let Err(e) = fs::create_dir_all(&db_dir) {
            panic!("Error creating database directory: {:?}", e);
        }

        let compact_profile = match self.raw_conf.db_compaction_profile.as_ref()
        {
            Some(p) => db::DatabaseCompactionProfile::from_str(p).unwrap(),
            None => db::DatabaseCompactionProfile::default(),
        };
        println!("{}", db_dir);
        db::db_config(
            Path::new(db_dir),
            self.raw_conf.db_cache_size.clone(),
            compact_profile,
            NUM_COLUMNS.clone(),
        )
    }
}

/// Validates and formats bootnodes option.
pub fn to_bootnodes(bootnodes: &Option<String>) -> Result<Vec<String>, String> {
    match *bootnodes {
        Some(ref x) if !x.is_empty() => x
            .split(',')
            .map(|s| match validate_node_url(s).map(Into::into) {
                None => Ok(s.to_owned()),
                Some(ErrorKind::AddressResolve(_)) => Err(format!(
                    "Failed to resolve hostname of a boot node: {}",
                    s
                )),
                Some(_) => Err(format!(
                    "Invalid node address format given for a boot node: {}",
                    s
                )),
            })
            .collect(),
        Some(_) => Ok(vec![]),
        None => Ok(vec![]),
    }
}
