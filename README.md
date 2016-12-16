# burst-rminer
Burstcoin miner written in rust. I wrote this so that I could mine on my odroid-xu4. 

To use:
1. install rust, cargo, and gcc
2. clone repo
3. edit config.json to point at your folders and pool 
4. inside repo: cargo build --release
5. cd target/release
6. burst-miner -config=../../config.json
