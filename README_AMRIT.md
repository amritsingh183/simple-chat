# Demo


The demo is on youtube at https://youtu.be/uPDw5o97Q9Y

- ***The Client CLI supports history with up,down arrow keys***
- ***The Client CLI supports cursor navigation with left,right arrow keys***
- ***Multiline input was not implemented***
- ***You can use any language for username, `stringzilla` ensures case-folding in any language, for case in-sensitive username uniqueness. `stringzilla` was chosen for its speed as well***
- Github actions tested locally using ***act push --job chat-e2e-test -P ubuntu-latest=catthehacker/ubuntu:full-latest***

The app is build using Rust 1.92.0

It has three components
- server `server`
- client `client`
- common `common` contains code common to `server` and `client`


The following files ensure same standards if the project is used by a team
- `rust-toolchain.toml`
- `rustfmt.toml`
- `.cargo/config.toml`


The app is very simple

- It has a buffered backbone called `Room` [crossbeam_channel::channel] which has a transmitter and receiver channel
- The client(s) sends commands over TCP to the server, the server parses those commands and sends messages over the buffered backbone transmitter via the broker (broker maintains a User registry)
- The broker has a dispatcher listening on backbone receiver and whatever it receives, it broadcasts to all user's via the user's specific buffered tx [crossbeam_channel::channel]
- The buffer ensure non-blocking communication



```
ingress = client->TCP->server->room_tx......->room_rx

egress = client<-TCP<-server<-user_rx<-......<-user_tx<-room_rx

```
## How to use the app

### Run the server

```bash
TZ="Asia/Kolkata" cargo run --bin server
```

### Run the client

```bash
cargo run -p client -- --username amrit
```

### Run another client

```bash
cargo run -p client -- --username alice
```


### In any of the clients

Send a message like so:

```bash
send "add your message here"
```

or

```bash
leave
```
