use crate::config::{Credentials, ElasticsearchInstance};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use std::collections::HashMap;
use std::time::Duration;

/// Manages HTTP clients for all configured ES instances.
#[derive(Debug)]
pub struct ConnectionManager {
    clients: HashMap<String, reqwest::Client>,
    instances: HashMap<String, ElasticsearchInstance>,
}

impl ConnectionManager {
    /// Create a ConnectionManager with HTTP clients for all instances.
    pub fn new(instances: Vec<ElasticsearchInstance>) -> anyhow::Result<Self> {
        let mut clients = HashMap::new();
        let mut instance_map = HashMap::new();

        for inst in instances {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

            let mut builder = reqwest::Client::builder()
                .default_headers(headers.clone())
                .timeout(Duration::from_secs(inst.default_timeout));

            // Credentials
            match &inst.credentials {
                Credentials::Basic { username, password } => {
                    // reqwest supports basic auth per-request, but we can set it via default headers
                    let auth = format!(
                        "Basic {}",
                        base64_encode(&format!("{}:{}", username, password))
                    );
                    let mut h = headers;
                    h.insert(AUTHORIZATION, HeaderValue::from_str(&auth)?);
                    builder = builder.default_headers(h);
                }
                Credentials::ApiKey { api_key } => {
                    let auth = format!("ApiKey {}", api_key);
                    let mut h = headers;
                    h.insert(AUTHORIZATION, HeaderValue::from_str(&auth)?);
                    builder = builder.default_headers(h);
                }
            }

            // SSL
            if let Some(ref ssl) = inst.ssl {
                if !ssl.verify_certs {
                    builder = builder.danger_accept_invalid_certs(true);
                }
                if let Some(ref ca_path) = ssl.ca_certs {
                    let ca_data = std::fs::read(ca_path)
                        .map_err(|e| anyhow::anyhow!("Failed to read CA cert {}: {}", ca_path, e))?;
                    let cert = reqwest::Certificate::from_pem(&ca_data)?;
                    builder = builder.add_root_certificate(cert);
                }
            }

            let client = builder.build()?;
            clients.insert(inst.name.clone(), client);
            instance_map.insert(inst.name.clone(), inst);
        }

        Ok(Self {
            clients,
            instances: instance_map,
        })
    }

    pub fn get_client(&self, name: &str) -> Result<&reqwest::Client, String> {
        self.clients.get(name).ok_or_else(|| {
            let available: Vec<&str> = self.clients.keys().map(|s| s.as_str()).collect();
            format!(
                "Unknown instance '{}'. Available: {}",
                name,
                available.join(", ")
            )
        })
    }

    pub fn get_instance_config(&self, name: &str) -> Result<&ElasticsearchInstance, String> {
        self.instances.get(name).ok_or_else(|| {
            let available: Vec<&str> = self.instances.keys().map(|s| s.as_str()).collect();
            format!(
                "Unknown instance '{}'. Available: {}",
                name,
                available.join(", ")
            )
        })
    }

    pub fn list_instances(&self) -> Vec<&ElasticsearchInstance> {
        self.instances.values().collect()
    }
}

fn base64_encode(input: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder =
            base64_writer::Base64Encoder::new(&mut buf, base64_writer::Variant::Standard);
        encoder.write_all(input.as_bytes()).unwrap();
        encoder.finish().unwrap();
    }
    String::from_utf8(buf).unwrap()
}

// Inline base64 encoder to avoid pulling in another crate
mod base64_writer {
    use std::io::{self, Write};

    const STANDARD: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    #[derive(Clone, Copy)]
    pub enum Variant {
        Standard,
    }

    pub struct Base64Encoder<'a, W: Write> {
        writer: &'a mut W,
        _variant: Variant,
        buf: [u8; 3],
        buf_len: usize,
    }

    impl<'a, W: Write> Base64Encoder<'a, W> {
        pub fn new(writer: &'a mut W, variant: Variant) -> Self {
            Self {
                writer,
                _variant: variant,
                buf: [0; 3],
                buf_len: 0,
            }
        }

        fn encode_block(&mut self, block: &[u8]) -> io::Result<()> {
            let table = STANDARD;
            match block.len() {
                3 => {
                    let b0 = block[0] as usize;
                    let b1 = block[1] as usize;
                    let b2 = block[2] as usize;
                    self.writer.write_all(&[
                        table[b0 >> 2],
                        table[((b0 & 0x03) << 4) | (b1 >> 4)],
                        table[((b1 & 0x0f) << 2) | (b2 >> 6)],
                        table[b2 & 0x3f],
                    ])
                }
                2 => {
                    let b0 = block[0] as usize;
                    let b1 = block[1] as usize;
                    self.writer.write_all(&[
                        table[b0 >> 2],
                        table[((b0 & 0x03) << 4) | (b1 >> 4)],
                        table[(b1 & 0x0f) << 2],
                        b'=',
                    ])
                }
                1 => {
                    let b0 = block[0] as usize;
                    self.writer.write_all(&[
                        table[b0 >> 2],
                        table[(b0 & 0x03) << 4],
                        b'=',
                        b'=',
                    ])
                }
                _ => Ok(()),
            }
        }

        pub fn finish(mut self) -> io::Result<()> {
            if self.buf_len > 0 {
                let block = self.buf[..self.buf_len].to_vec();
                self.encode_block(&block)?;
            }
            Ok(())
        }
    }

    impl<W: Write> Write for Base64Encoder<'_, W> {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            let mut pos = 0;
            while pos < data.len() {
                let needed = 3 - self.buf_len;
                let available = data.len() - pos;
                let to_copy = needed.min(available);
                self.buf[self.buf_len..self.buf_len + to_copy]
                    .copy_from_slice(&data[pos..pos + to_copy]);
                self.buf_len += to_copy;
                pos += to_copy;

                if self.buf_len == 3 {
                    self.encode_block(&self.buf.clone())?;
                    self.buf_len = 0;
                }
            }
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.writer.flush()
        }
    }
}
