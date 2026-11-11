//! Exclusive loopback attachment for the legacy command/data streams.
use std::{io, net::SocketAddr, time::Duration};

use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::timeout,
};

use super::{Channel, ModemPort, control_stream, data_stream};

const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Listener {
    control: TcpListener,
    data: TcpListener,
    attach_timeout: Duration,
}

impl Listener {
    /// These legacy sockets have no pairing credential. Limit deployment to
    /// one trusted local client, and keep the binding loopback-only.
    pub async fn bind(
        control: SocketAddr,
        data: SocketAddr,
        attach_timeout: Duration,
    ) -> io::Result<Self> {
        if !control.ip().is_loopback() || control.ip() != data.ip() || attach_timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "command and data listeners require the same loopback IP and a nonzero timeout",
            ));
        }
        Ok(Self {
            control: TcpListener::bind(control).await?,
            data: TcpListener::bind(data).await?,
            attach_timeout,
        })
    }

    pub fn addresses(&self) -> io::Result<(SocketAddr, SocketAddr)> {
        Ok((self.control.local_addr()?, self.data.local_addr()?))
    }

    async fn accept_pair(&self) -> io::Result<(TcpStream, TcpStream)> {
        let (first, control_first) = tokio::select! {
            socket = self.control.accept() => (socket?.0, true),
            socket = self.data.accept() => (socket?.0, false),
        };
        let (waiting, occupied) = if control_first {
            (&self.data, &self.control)
        } else {
            (&self.control, &self.data)
        };
        // A duplicate half never extends the attachment deadline or replaces
        // the accepted half. A half which has sent early bytes stays unread.
        let second = timeout(self.attach_timeout, async {
            loop {
                tokio::select! {
                    socket = occupied.accept() => drop(socket?),
                    socket = waiting.accept() => break Ok::<_, io::Error>(socket?.0),
                }
            }
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "host attachment expired"))??;
        Ok(if control_first {
            (first, second)
        } else {
            (second, first)
        })
    }

    /// Serve exactly one pair, then await cleanup before returning. The
    /// listener's exclusive borrow prevents concurrent pair owners.
    pub async fn serve_one(
        &mut self,
        port: &dyn ModemPort,
        notices: mpsc::Receiver<Vec<u8>>,
        received: mpsc::Receiver<Vec<u8>>,
    ) -> io::Result<()> {
        let (control, data) = self.accept_pair().await?;
        let (channel, result) = {
            let control = control_stream(control, port, notices, Some(WRITE_TIMEOUT));
            let data = data_stream(data, port, received, Some(WRITE_TIMEOUT));
            tokio::pin!(control, data);
            loop {
                tokio::select! {
                    biased;
                    result = &mut control => break (Channel::Control, result),
                    result = &mut data => break (Channel::Data, result),
                    socket = self.control.accept() => {
                        if let Err(error) = socket { break (Channel::Control, Err(error)); }
                    }
                    socket = self.data.accept() => {
                        if let Err(error) = socket { break (Channel::Data, Err(error)); }
                    }
                }
            }
        };
        // Both stream futures and sockets have been dropped. No write on a
        // stalled connection can delay ownership cleanup.
        port.closed(channel).await;
        port.closed(match channel {
            Channel::Control => Channel::Data,
            Channel::Data => Channel::Control,
        })
        .await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::Command, host::Reply};
    use async_trait::async_trait;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Port {
        echo: mpsc::Sender<Vec<u8>>,
        closed: Mutex<Vec<Channel>>,
    }

    #[async_trait]
    impl ModemPort for Port {
        async fn command(&self, command: Command) -> Reply {
            match command {
                Command::Version => Reply::Version("BENCH".into()),
                _ => Reply::Wrong,
            }
        }

        async fn data(&self, bytes: &[u8]) -> io::Result<()> {
            self.echo.try_send(bytes.to_vec()).map_err(io::Error::other)
        }

        async fn closed(&self, channel: Channel) {
            self.closed.lock().unwrap().push(channel);
        }
    }

    #[tokio::test]
    async fn exclusive_pair_accepts_data_first_and_closes_both_channels() {
        timeout(Duration::from_secs(3), async {
            let address = SocketAddr::from(([127, 0, 0, 1], 0));
            let mut listener = Listener::bind(address, address, Duration::from_secs(1))
                .await
                .unwrap();
            let (control_address, data_address) = listener.addresses().unwrap();
            let (_notice, notices) = mpsc::channel(4);
            let (echo, received) = mpsc::channel(4);
            let server = tokio::spawn(async move {
                let port = Port {
                    echo,
                    closed: Mutex::new(Vec::new()),
                };
                listener.serve_one(&port, notices, received).await.unwrap();
                port.closed.into_inner().unwrap()
            });
            let mut data = TcpStream::connect(data_address).await.unwrap();
            let mut control = TcpStream::connect(control_address).await.unwrap();
            control.write_all(b"VERSION\r").await.unwrap();
            let mut reply = [0; 14];
            control.read_exact(&mut reply).await.unwrap();
            assert_eq!(&reply, b"VERSION BENCH\r");

            let mut duplicate = TcpStream::connect(control_address).await.unwrap();
            let mut single = [0];
            match duplicate.read(&mut single).await {
                Ok(0) => {}
                Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
                result => panic!("unexpected duplicate connection result: {result:?}"),
            }

            let bytes = [0, 255, 13, 10, 1];
            data.write_all(&bytes).await.unwrap();
            let mut echo = [0; 5];
            data.read_exact(&mut echo).await.unwrap();
            assert_eq!(echo, bytes);
            drop(data);
            assert_eq!(control.read(&mut single).await.unwrap(), 0);
            let closed = server.await.unwrap();
            assert!(matches!(
                closed.as_slice(),
                [Channel::Data, Channel::Control]
            ));
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn sustained_data_drains_a_bounded_echo_queue() {
        timeout(Duration::from_secs(5), async {
            let address = SocketAddr::from(([127, 0, 0, 1], 0));
            let mut listener = Listener::bind(address, address, Duration::from_secs(1))
                .await
                .unwrap();
            let (control_address, data_address) = listener.addresses().unwrap();
            let (_notice, notices) = mpsc::channel(1);
            let (echo, received) = mpsc::channel(1);
            let server = tokio::spawn(async move {
                let port = Port {
                    echo,
                    closed: Mutex::new(Vec::new()),
                };
                listener.serve_one(&port, notices, received).await
            });

            let _control = TcpStream::connect(control_address).await.unwrap();
            let data = TcpStream::connect(data_address).await.unwrap();
            let (mut reader, mut writer) = data.into_split();
            let payload: Vec<_> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();
            let mut reply = vec![0; payload.len()];
            tokio::try_join!(writer.write_all(&payload), reader.read_exact(&mut reply)).unwrap();
            assert_eq!(reply, payload);
            drop((reader, writer));
            server.await.unwrap().unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn incomplete_pair_expires_without_entering_the_backend() {
        let address = SocketAddr::from(([127, 0, 0, 1], 0));
        let listener = Listener::bind(address, address, Duration::from_millis(25))
            .await
            .unwrap();
        let (control, _) = listener.addresses().unwrap();
        let mut half = TcpStream::connect(control).await.unwrap();
        let error = listener.accept_pair().await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(half.read(&mut [0]).await.unwrap(), 0);
    }
}
