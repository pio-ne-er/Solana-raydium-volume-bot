"use strict";
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.mainKp = exports.solanaConnection = void 0;
const spl_token_1 = require("@solana/spl-token");
const web3_js_1 = require("@solana/web3.js");
const constants_1 = require("./constants");
const utils_1 = require("./utils");
const bs58_1 = __importDefault(require("bs58"));
exports.solanaConnection = new web3_js_1.Connection(constants_1.RPC_ENDPOINT, {
    wsEndpoint: constants_1.RPC_WEBSOCKET_ENDPOINT,
});
exports.mainKp = web3_js_1.Keypair.fromSecretKey(bs58_1.default.decode(constants_1.PRIVATE_KEY));
const baseMint = new web3_js_1.PublicKey(constants_1.TOKEN_MINT);
const distritbutionNum = constants_1.DISTRIBUTE_WALLET_NUM > 20 ? 20 : constants_1.DISTRIBUTE_WALLET_NUM;
let quoteVault = null;
let poolKeys;
let sold = 0;
let bought = 0;
let totalSolPut = 0;
let changeAmount = 0;
let buyNum = 0;
let sellNum = 0;
utils_1.logger.level = constants_1.LOG_LEVEL;
const data = (0, utils_1.readJson)();
const walletPks = data.map(data => data.pubkey);
console.log("🚀 ~ walletPks:", walletPks);
const main = () => __awaiter(void 0, void 0, void 0, function* () {
    const solBalance = (yield exports.solanaConnection.getBalance(exports.mainKp.publicKey)) / web3_js_1.LAMPORTS_PER_SOL;
    console.log(`Wallet address: ${exports.mainKp.publicKey.toBase58()}`);
    console.log(`Pool token mint: ${baseMint.toBase58()}`);
    console.log(`Wallet SOL balance: ${solBalance.toFixed(3)}SOL`);
    console.log("Check interval: ", constants_1.CHECK_BAL_INTERVAL, "ms");
    let poolId;
    const poolKeys = yield utils_1.PoolKeys.fetchPoolKeyInfo(exports.solanaConnection, baseMint, spl_token_1.NATIVE_MINT);
    poolId = poolKeys.id;
    quoteVault = poolKeys.quoteVault;
    console.log(`Successfully fetched pool info`);
    console.log(`Pool id: ${poolId.toBase58()}`);
    trackWalletOnLog(exports.solanaConnection, quoteVault);
});
const getPoolStatus = (poolId) => __awaiter(void 0, void 0, void 0, function* () {
    while (true) {
        try {
            const res = yield fetch(`https://api.dexscreener.com/latest/dex/pairs/solana/${poolId === null || poolId === void 0 ? void 0 : poolId.toBase58()}`, {
                method: 'GET',
                headers: {
                    Accept: 'application/json',
                    'Content-Type': 'application/json'
                }
            });
            const data = yield res.json();
            const { url, priceNative, priceUsd, txns, volume, priceChange } = data.pair;
            // console.log(`\t url: ${url}`)
            // console.log(`\t price: ${priceNative} SOL / ${priceUsd} usd`)
            // console.log(`\t Volume status                  =>   m5: $${volume.m5}\t|\th1: $${volume.h1}\t|\th6: $${volume.h6}\t|\t h24: $${volume.h24}`)
            // console.log(`\t Recent buy status (buy / sell) =>   m5: ${txns.m5.buys} / ${txns.m5.sells}\t\t|\th1: ${txns.h1.buys} / ${txns.h1.sells}\t|\th6: ${txns.h6.buys} / ${txns.h6.sells}\t|\t h24: ${txns.h24.buys} / ${txns.h24.sells}`)
            // console.log(`\t volume price change            =>   m5: ${priceChange.m5}%\t\t|\th1: ${priceChange.h1}%\t|\th6: ${priceChange.h6}%\t|\t h24: ${priceChange.h24}%`)
            yield (0, utils_1.sleep)(5000);
        }
        catch (error) {
            console.log("Error fetching ");
            yield (0, utils_1.sleep)(2000);
        }
    }
});
function trackWalletOnLog(connection, quoteVault) {
    return __awaiter(this, void 0, void 0, function* () {
        const initialWsolBal = (yield connection.getTokenAccountBalance(quoteVault)).value.uiAmount;
        if (!initialWsolBal) {
            console.log("Quote vault mismatch");
            return;
        }
        const checkBal = setInterval(() => __awaiter(this, void 0, void 0, function* () {
            const bal = (yield connection.getTokenAccountBalance(quoteVault)).value.uiAmount;
            if (!bal) {
                console.log("Quote vault mismatch");
                return;
            }
            changeAmount = bal - initialWsolBal;
            (0, utils_1.deleteConsoleLines)(1);
            console.log(`Other users bought ${buyNum - bought} times and sold ${sellNum - sold} times, total SOL change is ${changeAmount - totalSolPut}SOL`);
        }), constants_1.CHECK_BAL_INTERVAL);
        try {
            connection.onLogs(quoteVault, (_a) => __awaiter(this, [_a], void 0, function* ({ logs, err, signature }) {
                var _b, _c;
                if (err) { }
                else {
                    const parsedData = yield connection.getParsedTransaction(signature, { maxSupportedTransactionVersion: 0, commitment: "confirmed" });
                    const signer = parsedData === null || parsedData === void 0 ? void 0 : parsedData.transaction.message.accountKeys.filter((elem) => {
                        return elem.signer == true;
                    })[0].pubkey.toBase58();
                    // console.log(`\nTransaction success: https://solscan.io/tx/${signature}\n`)
                    if (!walletPks.includes(signer)) {
                        if (Number((_b = parsedData === null || parsedData === void 0 ? void 0 : parsedData.meta) === null || _b === void 0 ? void 0 : _b.preBalances[0]) > Number((_c = parsedData === null || parsedData === void 0 ? void 0 : parsedData.meta) === null || _c === void 0 ? void 0 : _c.postBalances[0])) {
                            buyNum++;
                        }
                        else {
                            sellNum++;
                        }
                    }
                }
            }), "confirmed");
        }
        catch (error) { }
    });
}
main();
