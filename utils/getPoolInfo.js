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
Object.defineProperty(exports, "__esModule", { value: true });
exports.getPoolKeys = void 0;
const raydium_sdk_1 = require("@raydium-io/raydium-sdk");
const spl_token_1 = require("@solana/spl-token");
const web3_js_1 = require("@solana/web3.js");
function _formatAmmKeysById(id, connection) {
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
const getPoolKeys = (connection, baseMint) => __awaiter(void 0, void 0, void 0, function* () {
    try {
        const res = yield fetch(`https://api.dexscreener.com/latest/dex/tokens/${baseMint.toBase58()}`, {
            method: 'GET',
            headers: {
                Accept: 'application/json',
                'Content-Type': 'application/json'
            }
        });
        const data = yield res.clone().json();
        if (data.pairs.length == 0) {
            return null;
        }
        else {
            const raydiumPairId = data.pairs.filter((pair) => pair.dexId === "raydium" && pair.quoteToken.address == spl_token_1.NATIVE_MINT.toBase58())[0].pairAddress;
            const poolState = yield _formatAmmKeysById(raydiumPairId, connection);
            return poolState;
        }
    }
    catch (e) {
        console.log("error in fetching price of pool", e);
        return null;
    }
});
exports.getPoolKeys = getPoolKeys;
