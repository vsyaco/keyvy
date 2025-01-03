# Keyvy

A simple key-value store with TTL support.

## Usage

```
cargo run --release
```

## Commands

```
SET <KEY> <TTL?> <VALUE>
GET <KEY>
DEL <KEY>(, <KEY2>, ...)
```

## Example

```
SET mykey 10 hello
GET mykey
DEL mykey
```
