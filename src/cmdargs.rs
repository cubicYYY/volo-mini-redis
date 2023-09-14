use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct ServerConfig {
    /// Server name, used in default AOF file name.
    /// Will be randomly chosen if not provided
    #[arg(short, long)]
    pub name: Option<String>,

    /// Optional AOF file path
    #[arg(short, long, value_name = "FILE")]
    pub aof: Option<String>,

    /// Mark this Vodis instance as a slave
    #[arg(short, long, value_name = "Master IP:PORT")]
    pub slaveof: Option<String>,

    #[arg(short, long, value_name = "IP")]
    pub ip: String,

    #[arg(short, long, value_name = "port")]
    pub port: u16,

    /// Sets a custom config file
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub cluster: u8,

    /// Execute provided commands after initialization
    #[arg(long)]
    pub pre_run: Option<Vec<String>>,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct ClientConfig {
    /// Mark this Vodis instance as a slave
    #[arg(short, long, value_name = "Master IP:PORT")]
    pub slaveof: Option<String>,

    /// Execute provided commands after initialization
    #[arg(long)]
    pub pre_run: Option<Vec<String>>,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct ProxyConfig {
    /// Mark this Vodis instance as a slave
    #[arg(short, long, value_name = "Master IP:PORT")]
    pub attach_to: Option<String>,

    /// Cluster config file path
    #[arg(long)]
    pub cfg: Option<String>,
}