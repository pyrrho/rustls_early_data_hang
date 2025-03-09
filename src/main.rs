use std::{
    io::{ErrorKind, Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::Arc,
};

use eyre::Context;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tracing;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> eyre::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let certs = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("self_signed_certs")
        .join("cert.pem");
    let certs = CertificateDer::pem_file_iter(&certs)?.collect::<Result<Vec<_>, _>>()?;
    let key = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("self_signed_certs")
        .join("key.pem");
    let key = PrivateKeyDer::from_pem_file(&key)?;

    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    // COMMENT OUT THIS LINE and Firefox should consistently handle requests.
    server_config.max_early_data_size = 1024;

    let server_config = Arc::new(server_config);

    std::thread::scope(|s| -> eyre::Result<()> {
        s.spawn(move || -> eyre::Result<()> {
            let listener = std::net::TcpListener::bind("127.0.0.1:3000")?;
            tracing::info!("spawning www server on {listener:?}");

            loop {
                let (conn, peer_sa) = listener.accept()?;
                tracing::info!("serving connection from {peer_sa:?}");

                let tls = rustls::ServerConnection::new(server_config.clone())?;
                let tls = rustls::Connection::Server(tls);

                s.spawn(|| {
                    serve_once(conn, tls).context("conn serve failed").unwrap();
                });
            }
        });

        Ok(())
    })
}

fn serve_once(mut conn: TcpStream, mut tls: rustls::Connection) -> eyre::Result<()> {
    while tls.is_handshaking() {
        match tls.complete_io(&mut conn) {
            Ok(_) => {}
            Err(err) => {
                tracing::error!(?err, "complete_io failed");
                return Ok(());
            }
        };
    }

    let mut request = vec![0u8; 4096];
    let mut cursor = 0;
    loop {
        tracing::info!("Is the connection stuck?");
        tls.complete_io(&mut conn)?;
        tracing::info!("Nope!");
        let mut reader = tls.reader();
        let bytes_read = match reader.read(&mut request[cursor..]) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::WouldBlock => 0,
            otherwise => {
                otherwise.unwrap();
                0
            }
        };
        cursor += bytes_read;

        // lol trust the client
        if unsafe {
            std::str::from_utf8_unchecked(&request[..cursor])
                .lines()
                .last()
                == Some("")
        } {
            break;
        }
    }

    request.truncate(cursor);
    let request = String::from_utf8(request)?;

    // Check the requested path
    let request_line = request.lines().next().unwrap();
    if !request_line.starts_with("GET") {
        panic!("only GET is accepted");
    }

    let req = request_line
        .split(' ')
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let _span = tracing::info_span!("request", req);
    let _span = _span.enter();

    let path_segment = request_line
        .split(' ')
        .find(|seg| seg.starts_with('/'))
        .unwrap();

    let resp = match path_segment {
        "/" => index(),
        "/json" => json(),
        _otherwise => error(),
    };

    respond(resp, conn, tls)
}

#[tracing::instrument]
fn index() -> String {
    tracing::info!("generated reply");
    INDEX_HTML_TEMPLATE
        .replace("{content_len}", &format!("{}", INDEX_HTML.len() + 2))
        .replace("{index_html}", INDEX_HTML)
}

#[tracing::instrument]
fn json() -> String {
    tracing::info!("generated reply");
    JSON.to_string()
}

#[tracing::instrument]
fn error() -> String {
    tracing::info!("generated reply");
    ERROR.to_string()
}

#[tracing::instrument(skip_all)]
fn respond(response: String, mut conn: TcpStream, mut tls: rustls::Connection) -> eyre::Result<()> {
    tracing::info!("starting response");
    let mut buf = response.as_bytes();
    loop {
        if buf.is_empty() {
            tracing::info!("wrote full response");
            break;
        }

        tracing::info!("writing response chunk");
        while tls.wants_write() {
            tls.write_tls(&mut conn)?;
            conn.flush()?;
        }

        let mut writer = tls.writer();
        let bytes_written = writer.write(buf)?;
        buf = &buf[bytes_written..];
    }

    tracing::info!("sending closure notification");
    tls.complete_io(&mut conn)?;
    tls.send_close_notify();

    tracing::info!("flushing write buffer");
    tls.complete_io(&mut conn)?;

    conn.flush()?;

    Ok(())
}

#[rustfmt::skip]
const INDEX_HTML_TEMPLATE: &str = r#"HTTP/1.1 200 OK
content-type: text/html
content-length: {content_len}

{index_html}


"#;

#[rustfmt::skip]
const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html>

<head>
  <title>Axum Fetch Hang</title>
  <meta content="text/html;charset=utf-8" http-equiv="Content-Type" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <meta charset="UTF-8" />
</head>

<body>
  <p id="main">Please check the console for logs, and network tab for hanging requests.</p>
  <script lang="text/javascript">
    function fetchData() {
      return fetch("https://127.0.0.1:3000/json", {
        credentials: 'include'
      })
      .then(response => {
        if (!response.ok) {
          throw new Error('Network response was not ok');
        }
        return response.json();
      })
      .then(data => {
        console.log(data);
        return true;
      })
      .catch(error => {
        console.error('There was a problem with the fetch operation:', error);
        return false;
      });
    }

    fetchData();
    setTimeout(fetchData, 3000);
  </script>
</body>

</html>
"#;

#[rustfmt::skip]
const JSON: &str = r#"HTTP/1.1 200 OK
content-type: application/json

{
    "json": "object"
}

"#;

#[rustfmt::skip]
const ERROR: &str = r#"HTTP/1.1 500 INTERNAL SERVER ERROR
content-type: text/html

<html><body>
Something went wrong
</body></html>


"#;
