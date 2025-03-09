A minimal reproducer for an odd Firefox hang.

1. Run with `cargo run`
2. Open Firefox
3. Open the Network tab in the developer tools
4. Disable the browser cache
5. Navigate to https://127.0.0.1:3000
6. Acknowledge that these certs are self-signed, and load the page anyway
7. Possibly see some stalled requests to 127.0.0.1/json

The certificate handling is based off of Axum's `example-tls-rustls`;
https://github.com/tokio-rs/axum/tree/main/examples/tls-rustls
