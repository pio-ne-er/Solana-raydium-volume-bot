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
exports.getSellTxWithJupiter = exports.getBuyTxWithJupiter = exports.getSellTx = exports.getBuyTx = exports.formatAmmKeysById = void 0;
const assert_1 = __importDefault(require("assert"));
const raydium_sdk_1 = require("@raydium-io/raydium-sdk");
const web3_js_1 = require("@solana/web3.js");
const spl_token_1 = require("@solana/spl-token");
const constants_1 = require("../constants");
function getWalletTokenAccount(connection, wallet) {
    return __awaiter(this, void 0, void 0, function* () {
        const walletTokenAccount = yield connection.getTokenAccountsByOwner(wallet, {
            programId: spl_token_1.TOKEN_PROGRAM_ID,
        });
        return walletTokenAccount.value.map((i) => ({
            pubkey: i.pubkey,
            programId: i.account.owner,
            accountInfo: raydium_sdk_1.SPL_ACCOUNT_LAYOUT.decode(i.account.data),
        }));
    });
}
function swapOnlyAmm(connection, input) {
    return __awaiter(this, void 0, void 0, function* () {
        // -------- pre-action: get pool info --------
        const targetPoolInfo = yield formatAmmKeysById(connection, input.targetPool);
        (0, assert_1.default)(targetPoolInfo, 'cannot find the target pool');
        const poolKeys = (0, raydium_sdk_1.jsonInfo2PoolKeys)(targetPoolInfo);
        // -------- step 1: coumpute amount out --------
        const { amountOut, minAmountOut } = raydium_sdk_1.Liquidity.computeAmountOut({
            poolKeys: poolKeys,
            poolInfo: yield raydium_sdk_1.Liquidity.fetchInfo({ connection, poolKeys }),
            amountIn: input.inputTokenAmount,
            currencyOut: input.outputToken,
            slippage: input.slippage,
        });
        // -------- step 2: create instructions by SDK function --------
        const { innerTransactions } = yield raydium_sdk_1.Liquidity.makeSwapInstructionSimple({
            connection,
            poolKeys,
            userKeys: {
                tokenAccounts: input.walletTokenAccounts,
                owner: input.wallet.publicKey,
            },
            amountIn: input.inputTokenAmount,
            amountOut: minAmountOut,
            fixedSide: 'in',
            makeTxVersion: raydium_sdk_1.TxVersion.V0,
            computeBudgetConfig: {
                microLamports: 12000 * constants_1.TX_FEE,
                units: 100000
            }
        });
        return innerTransactions;
    });
}
function formatAmmKeysById(connection, id) {
    return __awaiter(this, void 0, void 0, function* () {
        const account = yield connection.getAccountInfo(new web3_js_1.PublicKey(id));
        if (account === null)
            throw Error(' get id info error ');
        const info = raydium_sdk_1.LIQUIDITY_STATE_LAYOUT_V4.decode(account.data);
        const marketId = info.marketId;
        const marketAccount = yield connection.getAccountInfo(marketId);
        if (marketAccount === null)
            throw Error(' get market info error');
        const marketInfo = raydium_sdk_1.MARKET_STATE_LAYOUT_V3.decode(marketAccount.data);
        const lpMint = info.lpMint;
        const lpMintAccount = yield connection.getAccountInfo(lpMint);
        if (lpMintAccount === null)
            throw Error(' get lp mint info error');
        const lpMintInfo = raydium_sdk_1.SPL_MINT_LAYOUT.decode(lpMintAccount.data);
        return {
            id,
            baseMint: info.baseMint.toString(),
            quoteMint: info.quoteMint.toString(),
            lpMint: info.lpMint.toString(),
            baseDecimals: info.baseDecimal.toNumber(),
            quoteDecimals: info.quoteDecimal.toNumber(),
            lpDecimals: lpMintInfo.decimals,
            version: 4,
            programId: account.owner.toString(),
            authority: raydium_sdk_1.Liquidity.getAssociatedAuthority({ programId: account.owner }).publicKey.toString(),
            openOrders: info.openOrders.toString(),
            targetOrders: info.targetOrders.toString(),
            baseVault: info.baseVault.toString(),
            quoteVault: info.quoteVault.toString(),
            withdrawQueue: info.withdrawQueue.toString(),
            lpVault: info.lpVault.toString(),
            marketVersion: 3,
            marketProgramId: info.marketProgramId.toString(),
            marketId: info.marketId.toString(),
            marketAuthority: raydium_sdk_1.Market.getAssociatedAuthority({ programId: info.marketProgramId, marketId: info.marketId }).publicKey.toString(),
            marketBaseVault: marketInfo.baseVault.toString(),
            marketQuoteVault: marketInfo.quoteVault.toString(),
            marketBids: marketInfo.bids.toString(),
            marketAsks: marketInfo.asks.toString(),
            marketEventQueue: marketInfo.eventQueue.toString(),
            lookupTableAccount: web3_js_1.PublicKey.default.toString()
        };
    });
}
exports.formatAmmKeysById = formatAmmKeysById;
function getBuyTx(solanaConnection, wallet, baseMint, quoteMint, amount, targetPool) {
    return __awaiter(this, void 0, void 0, function* () {
        const baseInfo = yield (0, spl_token_1.getMint)(solanaConnection, baseMint);
        if (baseInfo == null) {
            return null;
        }
        const baseDecimal = baseInfo.decimals;
        const baseToken = new raydium_sdk_1.Token(spl_token_1.TOKEN_PROGRAM_ID, baseMint, baseDecimal);
        const quoteToken = new raydium_sdk_1.Token(spl_token_1.TOKEN_PROGRAM_ID, quoteMint, 9);
        const quoteTokenAmount = new raydium_sdk_1.TokenAmount(quoteToken, Math.floor(amount * 10 ** 9));
        const slippage = new raydium_sdk_1.Percent(100, 100);
        const walletTokenAccounts = yield getWalletTokenAccount(solanaConnection, wallet.publicKey);
        const instructions = yield swapOnlyAmm(solanaConnection, {
            outputToken: baseToken,
            targetPool,
            inputTokenAmount: quoteTokenAmount,
            slippage,
            walletTokenAccounts,
            wallet: wallet,
        });
        const willSendTx = (yield (0, raydium_sdk_1.buildSimpleTransaction)({
            connection: solanaConnection,
            makeTxVersion: raydium_sdk_1.TxVersion.V0,
            payer: wallet.publicKey,
            innerTransactions: instructions,
            addLookupTableInfo: raydium_sdk_1.LOOKUP_TABLE_CACHE
        }))[0];
        if (willSendTx instanceof web3_js_1.VersionedTransaction) {
            willSendTx.sign([wallet]);
            return willSendTx;
        }
        return null;
    });
}
exports.getBuyTx = getBuyTx;
function getSellTx(solanaConnection, wallet, baseMint, quoteMint, amount, targetPool) {
    return __awaiter(this, void 0, void 0, function* () {
        try {
            const tokenAta = yield (0, spl_token_1.getAssociatedTokenAddress)(baseMint, wallet.publicKey);
            const tokenBal = yield solanaConnection.getTokenAccountBalance(tokenAta);
            if (!tokenBal || tokenBal.value.uiAmount == 0)
                return null;
            const balance = tokenBal.value.amount;
            tokenBal.value.decimals;
            const baseToken = new raydium_sdk_1.Token(spl_token_1.TOKEN_PROGRAM_ID, baseMint, tokenBal.value.decimals);
            const quoteToken = new raydium_sdk_1.Token(spl_token_1.TOKEN_PROGRAM_ID, quoteMint, 9);
            const baseTokenAmount = new raydium_sdk_1.TokenAmount(baseToken, amount);
            const slippage = new raydium_sdk_1.Percent(99, 100);
            const walletTokenAccounts = yield getWalletTokenAccount(solanaConnection, wallet.publicKey);
            const instructions = yield swapOnlyAmm(solanaConnection, {
                outputToken: quoteToken,
                targetPool,
                inputTokenAmount: baseTokenAmount,
                slippage,
                walletTokenAccounts,
                wallet: wallet,
            });
            const willSendTx = (yield (0, raydium_sdk_1.buildSimpleTransaction)({
                connection: solanaConnection,
                makeTxVersion: raydium_sdk_1.TxVersion.V0,
                payer: wallet.publicKey,
                innerTransactions: instructions,
                addLookupTableInfo: raydium_sdk_1.LOOKUP_TABLE_CACHE
            }))[0];
            if (willSendTx instanceof web3_js_1.VersionedTransaction) {
                willSendTx.sign([wallet]);
                return willSendTx;
            }
            return null;
        }
        catch (error) {
            console.log("Error in selling token");
            return null;
        }
    });
}
exports.getSellTx = getSellTx;
const getBuyTxWithJupiter = (wallet, baseMint, amount) => __awaiter(void 0, void 0, void 0, function* () {
    try {
        const lamports = Math.floor(amount * 10 ** 9);
        const quoteResponse = yield (yield fetch(`https://quote-api.jup.ag/v6/quote?inputMint=So11111111111111111111111111111111111111112&outputMint=${baseMint.toBase58()}&amount=${lamports}&slippageBps=100`)).json();
        // get serialized transactions for the swap
        const { swapTransaction } = yield (yield fetch("https://quote-api.jup.ag/v6/swap", {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({
                quoteResponse,
                userPublicKey: wallet.publicKey.toString(),
                wrapAndUnwrapSol: true,
                dynamicComputeUnitLimit: true,
                prioritizationFeeLamports: 52000
            }),
        })).json();
        // deserialize the transaction
        const swapTransactionBuf = Buffer.from(swapTransaction, "base64");
        var transaction = web3_js_1.VersionedTransaction.deserialize(swapTransactionBuf);
        // sign the transaction
        transaction.sign([wallet]);
        return transaction;
    }
    catch (error) {
        console.log("Failed to get buy transaction");
        return null;
    }
});
exports.getBuyTxWithJupiter = getBuyTxWithJupiter;
const getSellTxWithJupiter = (wallet, baseMint, amount) => __awaiter(void 0, void 0, void 0, function* () {
    try {
        const quoteResponse = yield (yield fetch(`https://quote-api.jup.ag/v6/quote?inputMint=${baseMint.toBase58()}&outputMint=So11111111111111111111111111111111111111112&amount=${amount}&slippageBps=100`)).json();
        // get serialized transactions for the swap
        const { swapTransaction } = yield (yield fetch("https://quote-api.jup.ag/v6/swap", {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify({
                quoteResponse,
                userPublicKey: wallet.publicKey.toString(),
                wrapAndUnwrapSol: true,
                dynamicComputeUnitLimit: true,
                prioritizationFeeLamports: 52000
            }),
        })).json();
        // deserialize the transaction
        const swapTransactionBuf = Buffer.from(swapTransaction, "base64");
        var transaction = web3_js_1.VersionedTransaction.deserialize(swapTransactionBuf);
        // sign the transaction
        transaction.sign([wallet]);
        return transaction;
    }
    catch (error) {
        console.log("Failed to get sell transaction");
        return null;
    }
});
exports.getSellTxWithJupiter = getSellTxWithJupiter;
