#!/bin/sh

set -e

export PROTOC_INCLUDE=${PWD}/../

# Get example icrc1 canister
if [ ! -f ic-payments/tests/common/ic-icrc1-ledger.wasm ]; then
    export IC_VERSION=b43543ce7365acd1720294e701e8e8361fa30c8f
    curl -o ic-icrc1-ledger.wasm.gz https://download.dfinity.systems/ic/${IC_VERSION}/canisters/ic-icrc1-ledger.wasm.gz
    gunzip ic-icrc1-ledger.wasm.gz
    mv ic-icrc1-ledger.wasm ic-payments/tests/common/
fi

# Build test payment canister
cargo build --target wasm32-unknown-unknown --features export-api -p test-payment-canister --release
ic-wasm target/wasm32-unknown-unknown/release/test_payment_canister.wasm -o ic-payments/tests/common/payment_canister.wasm shrink
