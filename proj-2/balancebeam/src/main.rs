mod request;
mod response;

use clap::Clap;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::stream::StreamExt;
use tokio::sync::Mutex;
use tokio::time::{delay_for, Duration};

/// Contains information parsed from the command-line invocation of balancebeam. The Clap macros
/// provide a fancy way to automatically construct a command-line argument parser.
#[derive(Clap, Debug)]
#[clap(about = "Fun with load balancing")]
struct CmdOptions {
    #[clap(
        short,
        long,
        about = "IP/port to bind to",
        default_value = "0.0.0.0:1100"
    )]
    bind: String,
    #[clap(short, long, about = "Upstream host to forward requests to")]
    upstream: Vec<String>,
    #[clap(
        long,
        about = "Perform active health checks on this interval (in seconds)",
        default_value = "10"
    )]
    active_health_check_interval: usize,
    #[clap(
        long,
        about = "Path to send request to for active health checks",
        default_value = "/"
    )]
    active_health_check_path: String,
    #[clap(
        long,
        about = "Maximum number of requests to accept per IP per minute (0 = unlimited)",
        default_value = "0"
    )]
    max_requests_per_minute: usize,
}

/// Contains information about the state of balancebeam (e.g. what servers we are currently proxying
/// to, what servers have failed, rate limiting counts, etc.)
///
/// You should add fields to this struct in later milestones.
struct ProxyState {
    /// How frequently we check whether upstream servers are alive (Milestone 4)
    #[allow(dead_code)]
    active_health_check_interval: usize,
    /// Where we should send requests when doing active health checks (Milestone 4)
    #[allow(dead_code)]
    active_health_check_path: String,
    /// Maximum number of requests an individual IP can make in a minute (Milestone 5)
    #[allow(dead_code)]
    max_requests_per_minute: usize,
    /// Addresses of servers that we are proxying to
    upstream_addresses: Vec<String>,
    /// Health Status of servers that we are proxying to_string
    upstream_status: Mutex<Vec<bool>>,
    /// Request counter per ip_addr
    rate_limit_counter: Mutex<HashMap<String, usize>>,
}

#[tokio::main]
async fn main() {
    // Initialize the logging library. You can print log messages using the `log` macros:
    // https://docs.rs/log/0.4.8/log/ You are welcome to continue using print! statements; this
    // just looks a little prettier.
    if let Err(_) = std::env::var("RUST_LOG") {
        std::env::set_var("RUST_LOG", "debug");
    }
    pretty_env_logger::init();

    // Parse the command line arguments passed to this program
    let options = CmdOptions::parse();
    if options.upstream.len() < 1 {
        log::error!("At least one upstream server must be specified using the --upstream option.");
        std::process::exit(1);
    }

    // Start listening for connections
    let mut listener = match TcpListener::bind(&options.bind).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("Could not bind to {}: {}", options.bind, err);
            std::process::exit(1);
        }
    };
    log::info!("Listening for requests on {}", options.bind);

    let upstream_num = options.upstream.len();
    // Handle incoming connections
    let state = ProxyState {
        upstream_addresses: options.upstream,
        active_health_check_interval: options.active_health_check_interval,
        active_health_check_path: options.active_health_check_path,
        max_requests_per_minute: options.max_requests_per_minute,
        upstream_status: Mutex::new(vec![true; upstream_num]),
        rate_limit_counter: Mutex::new(HashMap::new()),
    };

    let shared_state = Arc::new(state);

    let shared_state_clone = shared_state.clone();
    tokio::spawn(async move {
        active_health_check(shared_state_clone).await;
    });

    if shared_state.max_requests_per_minute > 0 {
        let shared_state_clone = shared_state.clone();
        tokio::spawn(async move {
            refresh_rate_limit_counter(shared_state_clone).await;
        });
    }
    let mut incoming = listener.incoming();
    while let Some(stream) = incoming.next().await {
        match stream {
            Ok(stream) => {
                // Handle connection
                let shared_state_clone = shared_state.clone();
                tokio::spawn(async move {
                    handle_connection(stream, shared_state_clone).await;
                });
            }
            Err(_) => {
                break;
            }
        }
    }
}

async fn choose_health_upstream_randomly(state: &Arc<ProxyState>) -> Option<usize> {
    loop {
        let mut rng = rand::rngs::StdRng::from_entropy();
        let upstream_status = state.upstream_status.lock().await;
        let upstream_idx = rng.gen_range(0, upstream_status.len());
        if upstream_status[upstream_idx] {
            return Some(upstream_idx);
        }
    }
}

async fn connect_to_upstream(state: Arc<ProxyState>) -> Result<TcpStream, std::io::Error> {
    // TODO: implement failover (milestone 3)
    loop {
        if let Some(upstream_idx) = choose_health_upstream_randomly(&state).await {
            let upstream_ip = &state.upstream_addresses[upstream_idx];
            match TcpStream::connect(upstream_ip).await {
                Ok(upstream) => return Ok(upstream),
                Err(_) => {
                    log::info!(
                        "Failed to connect to upstream {}: this server is dead",
                        upstream_ip
                    );
                    let mut upstream_status = state.upstream_status.lock().await;
                    upstream_status[upstream_idx] = false;
                    continue;
                }
            }
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "All servers are dead",
            ));
        };
    }
}

async fn send_response(client_conn: &mut TcpStream, response: &http::Response<Vec<u8>>) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!(
        "{} <- {}",
        client_ip,
        response::format_response_line(&response)
    );
    if let Err(error) = response::write_to_stream(&response, client_conn).await {
        log::warn!("Failed to send response to client: {}", error);
        return;
    }
}

async fn handle_connection(mut client_conn: TcpStream, state: Arc<ProxyState>) {
    if !check_rate_limit_counter(&mut client_conn, &state).await {
        return;
    }
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("Connection received from {}", client_ip);

    // Open a connection to a random destination server
    let mut upstream_conn = match connect_to_upstream(state).await {
        Ok(stream) => stream,
        Err(_error) => {
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
    };
    let upstream_ip = client_conn.peer_addr().unwrap().ip().to_string();

    // The client may now send us one or more requests. Keep trying to read requests until the
    // client hangs up or we get an error.
    loop {
        // Read a request from the client
        let mut request = match request::read_from_stream(&mut client_conn).await {
            Ok(request) => request,
            // Handle case where client closed connection and is no longer sending requests
            Err(request::Error::IncompleteRequest(0)) => {
                log::debug!("Client finished sending requests. Shutting down connection");
                return;
            }
            // Handle I/O error in reading from the client
            Err(request::Error::ConnectionError(io_err)) => {
                log::info!("Error reading request from client stream: {}", io_err);
                return;
            }
            Err(error) => {
                log::debug!("Error parsing request: {:?}", error);
                let response = response::make_http_error(match error {
                    request::Error::IncompleteRequest(_)
                    | request::Error::MalformedRequest(_)
                    | request::Error::InvalidContentLength
                    | request::Error::ContentLengthMismatch => http::StatusCode::BAD_REQUEST,
                    request::Error::RequestBodyTooLarge => http::StatusCode::PAYLOAD_TOO_LARGE,
                    request::Error::ConnectionError(_) => http::StatusCode::SERVICE_UNAVAILABLE,
                });
                send_response(&mut client_conn, &response).await;
                continue;
            }
        };
        log::info!(
            "{} -> {}: {}",
            client_ip,
            upstream_ip,
            request::format_request_line(&request)
        );

        // Add X-Forwarded-For header so that the upstream server knows the client's IP address.
        // (We're the ones connecting directly to the upstream server, so without this header, the
        // upstream server will only know our IP, not the client's.)
        request::extend_header_value(&mut request, "x-forwarded-for", &client_ip);

        // Forward the request to the server
        if let Err(error) = request::write_to_stream(&request, &mut upstream_conn).await {
            log::error!(
                "Failed to send request to upstream {}: {}",
                upstream_ip,
                error
            );
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
        log::debug!("Forwarded request to server");

        // Read the server's response
        let response = match response::read_from_stream(&mut upstream_conn, request.method()).await
        {
            Ok(response) => response,
            Err(error) => {
                log::error!("Error reading response from server: {:?}", error);
                let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
                send_response(&mut client_conn, &response).await;
                return;
            }
        };
        // Forward the response to the client
        send_response(&mut client_conn, &response).await;
        log::debug!("Forwarded response to client");
    }
}

async fn check_server(upstream_idx: usize, state: &Arc<ProxyState>) -> bool {
    let upstream_ip = &state.upstream_addresses[upstream_idx];
    let request = http::Request::builder()
        .method(http::Method::GET)
        .uri(&state.active_health_check_path)
        .header("Host", upstream_ip)
        .body(Vec::new())
        .unwrap();

    let mut upstream_conn = match connect_to_specify_server(upstream_ip).await {
        Ok(stream) => stream,
        Err(_error) => {
            return false;
        }
    };
    if let Err(error) = request::write_to_stream(&request, &mut upstream_conn).await {
        log::debug!(
            "Failed to send request to upsteram {}: {}",
            upstream_ip,
            error
        );
        return false;
    }
    match response::read_from_stream(&mut upstream_conn, request.method()).await {
        Ok(resp) if resp.status() == http::StatusCode::OK => return true,
        Ok(_resp) => {
            log::info!("Error the server is not healthy");
            return false;
        }
        Err(error) => {
            log::info!("Error reading response from server: {:?}", error);
            return false;
        }
    }
}

async fn active_health_check(state: Arc<ProxyState>) {
    let internal = state.active_health_check_interval as u64;
    loop {
        delay_for(Duration::from_secs(internal)).await;
        let mut upstream_status = state.upstream_status.lock().await;
        for upstream_idx in 0..upstream_status.len() {
            if check_server(upstream_idx, &state).await {
                upstream_status[upstream_idx] = true;
            } else {
                upstream_status[upstream_idx] = false;
            }
        }
    }
}

// connect to the specified server
async fn connect_to_specify_server(upstream_ip: &str) -> Result<TcpStream, std::io::Error> {
    match TcpStream::connect(upstream_ip).await {
        Ok(upstream) => return Ok(upstream),
        Err(err) => {
            log::info!("Failed to connect to upstream {}: {}", upstream_ip, err);
            return Err(err);
        }
    }
}

// Refreash rate limit counter
async fn refresh_rate_limit_counter(state: Arc<ProxyState>) {
    delay_for(Duration::from_secs(
        state.active_health_check_interval as u64,
    ))
    .await;
    let mut rate_limit_counter = state.rate_limit_counter.lock().await;
    rate_limit_counter.clear();
}

async fn check_rate_limit_counter(client_conn: &mut TcpStream, state: &Arc<ProxyState>) -> bool {
    let ip_addr = client_conn.peer_addr().unwrap().ip().to_string();
    let mut rate_limit_counter = state.rate_limit_counter.lock().await;
    let count = rate_limit_counter.entry(ip_addr.to_string()).or_insert(0);
    *count += 1;
    log::info!("{} requests from ip: {}", count, ip_addr);
    if *count > state.max_requests_per_minute {
        let response = response::make_http_error(http::StatusCode::TOO_MANY_REQUESTS);
        send_response(client_conn, &response).await;
        return false;
    }
    return true;
}
