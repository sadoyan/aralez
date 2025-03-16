use crate::utils::tools::*;
use crate::web::proxyhttp::LB;
// use clap::{arg, Parser};
use dashmap::DashMap;
use pingora_core::prelude::{background_service, Opt};
use pingora_core::server::Server;
use std::sync::Arc;

// #[derive(Parser, Debug)]
// #[command(version, about, long_about = None)]
// struct Args {
//     #[arg(short, long)]
//     address: String,
//     #[arg(short, long)]
//     port: String,
// }

pub fn run() {
    env_logger::init();

    // let mut server = Server::new(None).unwrap();
    let mut server = Server::new(Some(Opt::parse_args())).unwrap();
    server.bootstrap();

    let uf: UpstreamsDashMap = DashMap::new();
    let ff: UpstreamsDashMap = DashMap::new();
    let uf_config = Arc::new(uf);
    let ff_config = Arc::new(ff);

    let lb = LB {
        ump_upst: uf_config.clone(),
        ump_full: ff_config.clone(),
    };
    let bg = LB {
        ump_upst: uf_config.clone(),
        ump_full: ff_config.clone(),
    };

    let bg_srvc = background_service("bgsrvc", bg);
    let mut proxy = pingora_proxy::http_proxy_service(&server.configuration, lb);

    // let args = Args::parse();
    // let addr = format!("{}:{}", args.address, args.port);
    proxy.add_tcp("0.0.0.0:6193");
    server.add_service(proxy);
    server.add_service(bg_srvc);

    // info!("Starting Gazan server on {}, port : {} !", args.address, args.port);

    server.run_forever();
}
