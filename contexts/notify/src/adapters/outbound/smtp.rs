use std::{io, time::Duration};

use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

use crate::application::{DeliveryError, DeliveryReceipt, EmailGateway, RenderedEmail};

#[derive(Clone)]
pub struct SmtpEmailGateway {
    address: String,
    from: String,
    timeout: Duration,
}

impl SmtpEmailGateway {
    pub fn new(address: String, from: String, timeout: Duration) -> Result<Self, &'static str> {
        if address.is_empty()
            || address.contains(['\r', '\n'])
            || from.is_empty()
            || from.contains(['\r', '\n'])
            || timeout.is_zero()
        {
            return Err("invalid SMTP configuration");
        }
        Ok(Self {
            address,
            from,
            timeout,
        })
    }

    pub async fn ready(&self) -> bool {
        timeout(self.timeout, async {
            let stream = TcpStream::connect(&self.address).await?;
            let (read, _) = stream.into_split();
            let mut read = BufReader::new(read);
            match read_response(&mut read).await {
                Ok((220, _)) => Ok(()),
                _ => Err(io::Error::other("SMTP greeting is not ready")),
            }
        })
        .await
        .is_ok_and(|result: Result<(), io::Error>| result.is_ok())
    }

    async fn exchange(&self, email: &RenderedEmail) -> Result<DeliveryReceipt, DeliveryError> {
        if email.to.contains(['\r', '\n'])
            || email.subject.contains(['\r', '\n'])
            || email.body.contains('\r')
        {
            return Err(DeliveryError::Permanent("invalid_message"));
        }
        let stream = TcpStream::connect(&self.address)
            .await
            .map_err(classify_io)?;
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        expect(&mut read, 220).await?;
        command(&mut read, &mut write, "EHLO gavel-notify\r\n", 250).await?;
        command(
            &mut read,
            &mut write,
            &format!("MAIL FROM:<{}>\r\n", self.from),
            250,
        )
        .await?;
        command(
            &mut read,
            &mut write,
            &format!("RCPT TO:<{}>\r\n", email.to),
            250,
        )
        .await?;
        command(&mut read, &mut write, "DATA\r\n", 354).await?;
        let body = email.body.replace('\n', "\r\n").replace("\r\n.", "\r\n..");
        write.write_all(format!("From: {}\r\nTo: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n.\r\n",self.from,email.to,email.subject,body).as_bytes()).await.map_err(classify_io)?;
        let response = read_response(&mut read).await?;
        if response.0 != 250 {
            return Err(classify_code(response.0));
        }
        let _ = write.write_all(b"QUIT\r\n").await;
        Ok(DeliveryReceipt {
            provider_reference: response.1,
        })
    }
}

#[async_trait]
impl EmailGateway for SmtpEmailGateway {
    async fn deliver(&self, email: &RenderedEmail) -> Result<DeliveryReceipt, DeliveryError> {
        timeout(self.timeout, self.exchange(email))
            .await
            .map_err(|_| DeliveryError::Transient("smtp_timeout"))?
    }
}

async fn command(
    read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    write: &mut tokio::net::tcp::OwnedWriteHalf,
    value: &str,
    expected: u16,
) -> Result<(), DeliveryError> {
    write
        .write_all(value.as_bytes())
        .await
        .map_err(classify_io)?;
    let (code, _) = read_response(read).await?;
    if code == expected {
        Ok(())
    } else {
        Err(classify_code(code))
    }
}
async fn expect(
    read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    expected: u16,
) -> Result<(), DeliveryError> {
    let (code, _) = read_response(read).await?;
    if code == expected {
        Ok(())
    } else {
        Err(classify_code(code))
    }
}
async fn read_response(
    read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Result<(u16, String), DeliveryError> {
    let mut full = String::new();
    loop {
        let mut line = String::new();
        let bytes = read.read_line(&mut line).await.map_err(classify_io)?;
        if bytes == 0 {
            return Err(DeliveryError::Transient("smtp_eof"));
        }
        if full.len() + line.len() > 4096 {
            return Err(DeliveryError::Permanent("smtp_response_too_large"));
        }
        let code = line
            .get(0..3)
            .and_then(|v| v.parse().ok())
            .ok_or(DeliveryError::Permanent("smtp_invalid_response"))?;
        let more = line.as_bytes().get(3) == Some(&b'-');
        full.push_str(line.trim());
        if !more {
            return Ok((code, full));
        }
    }
}
fn classify_io(error: io::Error) -> DeliveryError {
    match error.kind() {
        io::ErrorKind::InvalidInput => DeliveryError::Permanent("smtp_invalid_input"),
        _ => DeliveryError::Transient("smtp_unavailable"),
    }
}
fn classify_code(code: u16) -> DeliveryError {
    if (400..500).contains(&code) {
        DeliveryError::Transient("smtp_temporary_rejection")
    } else {
        DeliveryError::Permanent("smtp_rejected")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[test]
    fn smtp_codes_are_bounded_and_safe() {
        assert_eq!(
            classify_code(421),
            DeliveryError::Transient("smtp_temporary_rejection")
        );
        assert_eq!(
            classify_code(550),
            DeliveryError::Permanent("smtp_rejected")
        );
    }

    #[tokio::test]
    async fn mailpit_compatible_dialogue_delivers_plain_text() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut read = BufReader::new(read);
            write.write_all(b"220 mailpit ready\r\n").await.unwrap();
            let mut message = String::new();
            let mut in_data = false;
            loop {
                let mut line = String::new();
                if read.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                if in_data {
                    if line == ".\r\n" {
                        write.write_all(b"250 queued-as-test\r\n").await.unwrap();
                        in_data = false;
                    } else {
                        message.push_str(&line);
                    }
                } else if line.starts_with("DATA") {
                    write.write_all(b"354 continue\r\n").await.unwrap();
                    in_data = true;
                } else if line.starts_with("QUIT") {
                    write.write_all(b"221 bye\r\n").await.unwrap();
                    break;
                } else {
                    write.write_all(b"250 ok\r\n").await.unwrap();
                }
            }
            message
        });
        let gateway = SmtpEmailGateway::new(
            address.to_string(),
            "no-reply@gavel.local".into(),
            Duration::from_secs(1),
        )
        .unwrap();
        let receipt = gateway
            .deliver(&RenderedEmail {
                to: "winner@gavel.local".into(),
                subject: "Покупка подтверждена".into(),
                body: "Аукцион a-1 завершён".into(),
            })
            .await
            .unwrap();
        assert!(receipt.provider_reference.contains("queued-as-test"));
        let message = server.await.unwrap();
        assert!(message.contains("To: winner@gavel.local"));
        assert!(message.contains("Аукцион a-1 завершён"));
    }
}
