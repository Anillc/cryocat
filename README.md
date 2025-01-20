# cryocat

A program that pipes stdio to the other side with STUN/TURN.

## usage

### server

```sh
cryocat-server -b 0.0.0.0:80
```

### client (same at both side)

```sh
export SERVER=ws://server.example.com
export STUN=stun.example.com
export TURN=turn.example.com
export TURN_USERNAME=username
export TURN_CREDENTIAL=credential
cryocat some-id
```
