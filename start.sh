rm -rf genesis
mkdir genesis
# export RUST_LOG=trace
cargo run node  --chain genesis.json \
    --datadir genesis \
    --http --auto-mine -d
