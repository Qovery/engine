use libtls::{config, error};
use qovery_engine::error::{SimpleError, SimpleErrorKind};
use std::io::{Read, Write};

pub fn is_tls_certificate_valid(domain: &str) -> error::Result<()> {
    let addr = &(domain.to_owned() + ":443");

    let request = format!(
        "GET / HTTP/1.1\r\n\
         Host: {}\r\n\
         Connection: close\r\n\r\n",
        domain
    );

    let mut tls = config::Builder::new().client()?;

    tls.connect(addr, None)?;
    tls.write_all(request.as_bytes())?;

    let mut buf = vec![0u8; 1024];
    tls.read_exact(&mut buf)?;

    let ok = b"HTTP/1.1 200 OK\r\n";
    assert_eq!(&buf[..ok.len()], ok);

    Ok(())
}

#[cfg(test)]
mod test_tls {
    use crate::tls::is_tls_certificate_valid;

    #[test]
    fn check_tls_certificate_validity() {
        //assert!(is_tls_certificate_valid("dns-do-not-exists.qovery.com").is_err());
        // assert!(is_tls_certificate_valid("self-signed.badssl.com").is_err());
        // assert!(is_tls_certificate_valid("expired.badssl.com").is_err());
        // assert!(is_tls_certificate_valid("revoked.badssl.com").is_err());
        assert!(is_tls_certificate_valid("www.google.com").is_ok())
    }
}
