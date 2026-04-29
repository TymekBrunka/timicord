use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Builder;

static MAX_HEADER_SIZE: usize = 1024 * 8;
static MAX_BODY_SIZE: usize = 1024 * 1024 * 60;

async fn read_until_full_get(
    socket: &mut TcpStream,
    _addr: SocketAddr,
    buf: &mut [u8],
    curlen: usize,
) -> Result<Vec<u8>, ()> {
    let mut data = Vec::<u8>::new();
    data.reserve_exact(curlen * 2);

    let mut n: usize = curlen;
    loop {
        if data.len() + n > MAX_HEADER_SIZE {
            return Err(());
        }

        data.extend_from_slice(&buf[0..n]);
        // println!("{} {} data: {:#?}", data.len(), n, String::from_utf8_lossy(&buf[0..n]));

        let a = data[0..data.len()]
            .windows(4)
            .rposition(|window| window == b"\r\n\r\n");
        if a.is_some() {
            data.resize_with(a.unwrap(), || 0);
            return Ok(data);
        }

        let _n = socket.read(buf).await;
        if let Ok(0) = _n {
            return Err(());
        }
        if let Err(_) = _n {
            return Err(());
        }
        n = _n.unwrap();
    }
}

async fn read_until_full_post(
    socket: &mut TcpStream,
    _addr: SocketAddr,
    buf: &mut [u8],
    curlen: usize,
) -> Result<Vec<u8>, ()> {
    let mut data = Vec::<u8>::new();
    data.reserve_exact(curlen * 2);

    let mut n: usize = curlen;
    let mut found = false;
    let mut a: usize = 0;
    loop {
        if data.len() + n > MAX_HEADER_SIZE {
            return Err(());
        }

        data.extend_from_slice(&buf[0..n]);
        let _a = data[0..data.len()]
            .windows(4)
            .rposition(|window| window == b"\r\n\r\n");
        if _a.is_some() {
            a = _a.unwrap();
            found = true;
            break;
        }

        // println!("{} data: {:#?}", n, String::from_utf8_lossy(&buf[0..n]));
        let _n = socket.read(buf).await;
        if let Ok(0) = _n {
            return Err(());
        }
        if let Err(_) = _n {
            return Err(());
        }
        n = _n.unwrap();
    }

    if !found {
        return Err(());
    }

    let mut body = data.split_off(a);
    let _headers = data.split_off(4);
    let headers: Vec<&[u8]> = _headers.split(|i| i == &('\n' as u8)).collect();

    for header in &headers {
        println!("header: {}", String::from_utf8_lossy(&header));
    }

    let ctl_header = headers
        .iter()
        .rfind(|header| header.starts_with(b"Content-Length:"));

    if ctl_header.is_none() {
        return Err(());
    }

    let ctl_header = ctl_header.unwrap();

    let content_length = usize::from_str_radix(
        &str::from_utf8(ctl_header[16..].trim_ascii()).unwrap_or(""),
        10,
    );

    if content_length.is_err() {
        return Err(());
    }
    let content_length = content_length.unwrap();

    println!(
        "{:#?} the header: {}",
        content_length,
        String::from_utf8_lossy(ctl_header)
    );

    body = body.split_off(4); //get rid of \r\n\r\n

    if body.len() + content_length > MAX_BODY_SIZE {
        return Err(());
    }

    let blen = body.len();

    if (content_length - blen > 0) {
        body.reserve(content_length - blen);
        match socket.read(&mut body[blen..]).await {
            Ok(0) => Err(()),
            Ok(_) => {
                return Ok(body);
            }
            Err(_) => Err(()),
        }
    } else {
        body.resize_with(blen, || 0);
        return Ok(body);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:8000").await?;

    let request_runtime = Builder::new_multi_thread()
        .worker_threads(8)
        .thread_name("request-pool")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_time()
        .enable_io()
        .build()
        .unwrap();

    loop {
        let (mut socket, addr) = listener.accept().await?;

        let _g = request_runtime.enter();

        request_runtime.spawn(async move {
            let mut buf = [0; 1024];

            // In a loop, read data from the socket and write the data back.
            loop {
                let n = match socket.read(&mut buf).await {
                    // socket closed
                    Ok(0) => return,
                    Ok(n) => {
                        // println!(
                        //     "from {:#?} received: {}",
                        //     addr.ip(),
                        //     String::from_utf8_lossy(&buf[0..n])
                        // );
                        println!("haha: {:#?}", String::from_utf8_lossy(&buf[0..3]));
                        let mut data = Ok(Vec::<u8>::new());
                        if &buf[0..3] == b"GET" {
                            data = read_until_full_get(&mut socket, addr, &mut buf, n).await;
                        } else if &buf[0..4] == b"POST" {
                            data = read_until_full_post(&mut socket, addr, &mut buf, n).await;
                        }
                        if data.is_err() {
                            eprintln!("failed to read message");
                            return;
                        }
                        println!(
                            "From {} received: {}",
                            addr.ip(),
                            String::from_utf8_lossy(&data.unwrap()[..])
                        );
                        n
                    }
                    Err(e) => {
                        eprintln!("failed to read from socket; err = {:?}", e);
                        return;
                    }
                };

                // Write the data back
                if let Err(e) = socket.write_all(&buf[0..n]).await {
                    eprintln!("failed to write to socket; err = {:?}", e);
                    return;
                }
            }
        });
    }
}
