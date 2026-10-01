use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Parser)]
#[command(
    name = "chorus-desktop",
    version,
    about = "Chorus desktop and bundled node"
)]
pub struct Options {
    /// Run the bundled node in the terminal without opening a desktop window.
    #[arg(long)]
    pub headless: bool,
    /// Initialize a headless node, then exit. Passwords are prompted securely.
    #[arg(long, requires = "headless", conflicts_with_all = ["host", "port", "origin", "key_password_file", "key_password_stdin"])]
    pub init: bool,
    /// Data directory. Defaults to the same application data directory as the GUI.
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Admin username for --init (default: admin).
    #[arg(long, requires = "init")]
    pub username: Option<String>,
    /// Import an nsec using a hidden prompt during --init.
    #[arg(long, requires = "init", conflicts_with = "credentials_stdin")]
    pub import_nsec: bool,
    /// Protected keystore password file for headless service startup.
    #[arg(long, requires = "headless", conflicts_with = "key_password_stdin")]
    pub key_password_file: Option<PathBuf>,
    #[arg(long, hide = true, requires = "headless")]
    pub key_password_stdin: bool,
    #[arg(long, hide = true, requires = "init")]
    pub credentials_stdin: bool,
    /// Node listen address (IPv4 or IPv6). Defaults to 127.0.0.1.
    #[arg(long)]
    pub host: Option<IpAddr>,
    /// Node port. Defaults to 8787 with --host or --headless, otherwise an available port.
    #[arg(long)]
    pub port: Option<u16>,
    /// Browser origin, e.g. https://chorus.example.com. Required for remote listening.
    #[arg(long)]
    pub origin: Option<Url>,
}

pub struct Endpoint {
    pub bind: SocketAddr,
    pub health_url: String,
    pub origin: String,
}

impl Options {
    pub fn bind(&self) -> SocketAddr {
        SocketAddr::new(
            self.host.unwrap_or(Ipv4Addr::LOCALHOST.into()),
            self.port
                .unwrap_or(if self.host.is_some() || self.headless {
                    8787
                } else {
                    0
                }),
        )
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(origin) = &self.origin {
            ensure!(
                matches!(origin.scheme(), "http" | "https")
                    && origin.host_str().is_some()
                    && origin.username().is_empty()
                    && origin.password().is_none()
                    && origin.path() == "/"
                    && origin.query().is_none()
                    && origin.fragment().is_none(),
                "--origin must be an HTTP(S) origin without credentials, a path, query or fragment"
            );
        }
        ensure!(
            self.bind().ip().is_loopback()
                || self
                    .origin
                    .as_ref()
                    .is_some_and(|url| url.scheme() == "https"),
            "remote listening requires --origin https://your-domain (with an HTTPS reverse proxy)"
        );
        Ok(())
    }

    pub fn endpoint(&self) -> Result<Endpoint> {
        self.validate()?;
        let listener = TcpListener::bind(self.bind())
            .with_context(|| format!("cannot listen on {}", self.bind()))?;
        let bind = listener.local_addr()?;
        // Wildcard addresses are listen targets, not browser destinations.
        let connect_ip = match bind.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => Ipv4Addr::LOCALHOST.into(),
            IpAddr::V6(ip) if ip.is_unspecified() => Ipv6Addr::LOCALHOST.into(),
            ip => ip,
        };
        let health_url = format!("http://{}", SocketAddr::new(connect_ip, bind.port()));
        let origin = self.origin.as_ref().map_or_else(
            || health_url.clone(),
            |url| url.origin().ascii_serialization(),
        );
        Ok(Endpoint {
            bind,
            health_url,
            origin,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_explicit_ports() -> Result<()> {
        let default = Options::try_parse_from(["chorus"])?;
        assert_eq!(default.bind(), "127.0.0.1:0".parse()?);
        let explicit = Options::try_parse_from(["chorus", "--port", "8789"])?;
        assert_eq!(explicit.bind(), "127.0.0.1:8789".parse()?);
        let headless = Options::try_parse_from(["chorus", "--headless"])?;
        assert_eq!(headless.bind(), "127.0.0.1:8787".parse()?);
        assert!(Options::try_parse_from(["chorus", "--init"]).is_err());
        assert!(Options::try_parse_from(["chorus", "--headless", "--init"]).is_ok());
        assert!(Options::try_parse_from(["chorus", "--key-password-file", "secret"]).is_err());
        assert!(Options::try_parse_from(["chorus", "--port", "65536"]).is_err());
        assert!(Options::try_parse_from(["chorus", "--host", "invalid"]).is_err());
        Ok(())
    }

    #[test]
    fn remote_listeners_require_https() -> Result<()> {
        for host in ["0.0.0.0", "::", "192.0.2.1"] {
            let missing = Options::try_parse_from(["chorus", "--host", host])?;
            assert!(missing.validate().is_err());
            assert_eq!(missing.bind().port(), 8787);
            let http = Options::try_parse_from([
                "chorus",
                "--host",
                host,
                "--origin",
                "http://example.com",
            ])?;
            assert!(http.validate().is_err());
            let https = Options::try_parse_from([
                "chorus",
                "--host",
                host,
                "--origin",
                "https://example.com",
            ])?;
            https.validate()?;
        }
        Ok(())
    }

    #[test]
    fn origins_are_exact_and_wildcards_use_loopback_for_health() -> Result<()> {
        let options = Options::try_parse_from([
            "chorus",
            "--host",
            "0.0.0.0",
            "--port",
            "0",
            "--origin",
            "https://example.com:8443/",
        ])?;
        let endpoint = options.endpoint()?;
        assert_ne!(endpoint.bind.port(), 0);
        assert_eq!(endpoint.origin, "https://example.com:8443");
        assert_eq!(
            endpoint.health_url,
            format!("http://127.0.0.1:{}", endpoint.bind.port())
        );
        for origin in [
            "https://example.com/path",
            "https://u:p@example.com",
            "https://example.com/?q=1",
            "https://example.com/#x",
            "ftp://example.com",
        ] {
            assert!(
                Options::try_parse_from(["chorus", "--origin", origin])?
                    .validate()
                    .is_err()
            );
        }
        let occupied = TcpListener::bind("127.0.0.1:0")?;
        let options = Options::try_parse_from([
            "chorus",
            "--port",
            &occupied.local_addr()?.port().to_string(),
        ])?;
        assert!(options.endpoint().is_err());
        Ok(())
    }
}
