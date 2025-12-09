# How can I boost the volume of spl token in Raydium?
# Raydium Volume Bot

This volume bot distribute SOL to multiple wallets and buy and sell with that distributed wallets permanently on the Raydium platform.

https://x.com/Pio_ne_er/status/1841157402475053090

You can try version1 with this telegram bot

https://t.me/@Ifb_volumebot

This is the video for the users who don't know well about programming to facilitate the running the CLI bots.
I hope this is very helpful for those people.

## Contact Info

Telegram: @hi_3333

You can always feel free to find me here for my help on other projects.

## Features

- **Automated Wallet Creation**: Create number of wallets automatically to buy and sell the token
- **Automated SOL Distribution**: Distributes SOL to those new wallets.
- **Endless Buy and Sell Swaps**: Buy and Sell with those wallets permanently.
- **Configurable Parameters**: Allows customization of buy amounts, intervals, distribution settings, and more.

## Usage
1. Clone the repository
```
git clone https://github.com/pio-ne-er/Solana-raydium-volume-bot.git
cd Solana-raydium-volume-bot
```
2. Install dependencies
```
npm install
```
3. Configure the environment variables

Rename the .env.copy file to .env and set RPC and WSS, main wallet's secret key, and jito auth keypair.

4. Run the bot

```
npm run start
```

# Version 2 is developed and it is private repository.
### What is the main difference between the former volume booster and the updated one?

https://x.com/Pio_ne_er/status/1831053270737121302
You can see the transactions and bot running in this video.
I hope it will be helpful for you.

## 🚀 Last Version's Drawbacks and Improvements
- ❌ **Repetitive buy and sell with one wallet**: The last version of the Raydium Volume Bot used fixed wallets, so it was apparent on DexScreener that some wallets performed repetitive buy and sell actions.
- ✅ **Transferring SOL to new wallet**: After buying and selling in one wallet, it transfers SOL to a newly created wallet and continues buying and selling there.
- ❌ **No increase in the number of makers**: It didn't increase the number of pool makers, only the volume.
- ✅ **Maker increase**: New wallets are created every round of buying and selling, increasing the number of makers.
- ❌ **Gathering token instead of SOL**: When gathering, if there were tokens left, it didn't sell them before gathering. Instead, it just gathered tokens to the main wallet.
- ✅ **Sell before gather**: When gathering, if there are tokens left in the wallet, it sells the tokens first and gathers only SOL (the token account rent of 0.00203 SOL is reclaimed).
- ❌ **Equal number of buys and sells**: One-time buy and one-time sell actions left sell pressure at the end, as there was always a sell at the end of the volume operation.
- ✅ **More buys than sells**: It randomly buys twice with SOL in the wallet and sells all tokens after some time, making the number of buys twice as many as sells, thus creating more buy pressure.

# Version 3 is also developed.
### Version 3 is designed for the massive transactions in a very short time.
So, it is making 500 transactions in a minute.
You can see my volume bot version3 working in here.

https://dexscreener.com/solana/boisf5dnefrbsodmiupkpauglaggx9gdjl1csgmcjqnn

There is no repeated makers and it is making 20 transactions a second.

If you increase the volume of each transaction, 1 million in an hour is a piece of cake.
How wonderful it is.
I am proud of my version3.
If you need it, you can contact me.

