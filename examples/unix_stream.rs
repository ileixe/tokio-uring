use std::env;

use tokio_uring::{net::UnixStream, Submit};

fn main() {
    let args: Vec<_> = env::args().collect();

    if args.len() <= 1 {
        panic!("no addr specified");
    }

    let socket_addr: &String = &args[1];

    tokio_uring::start(async {
        let stream = UnixStream::connect(socket_addr).await.unwrap();
        let buf = vec![1u8; 128].into();

        let (n, buf) = stream.write(buf).submit().await.unwrap();
        println!("written: {}", n);

        let (read, buf) = stream.read(buf).await.unwrap();
        println!("read: {:?}", &buf[0][..read]);
    });
}
