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
exports.PoolKeys = void 0;
const raydium_sdk_1 = require("@raydium-io/raydium-sdk");
const web3_js_1 = require("@solana/web3.js");
const dotenv_1 = __importDefault(require("dotenv"));
dotenv_1.default.config();
class PoolKeys {
    static fetchMarketId(connection, baseMint, quoteMint, commitment) {
        return __awaiter(this, void 0, void 0, function* () {
            let accounts = yield connection.getProgramAccounts(new web3_js_1.PublicKey('srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX'), {
                commitment,
                filters: [
                    { dataSize: raydium_sdk_1.MARKET_STATE_LAYOUT_V3.span },
                    {
                        memcmp: {
                            offset: raydium_sdk_1.MARKET_STATE_LAYOUT_V3.offsetOf("baseMint"),
                            bytes: baseMint.toBase58(),
                        },
                    },
                    {
                        memcmp: {
                            offset: raydium_sdk_1.MARKET_STATE_LAYOUT_V3.offsetOf("quoteMint"),
                            bytes: quoteMint.toBase58(),
                        },
                    },
                ],
            });
            if (!accounts)
                accounts = yield connection.getProgramAccounts(new web3_js_1.PublicKey('srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX'), {
                    commitment,
                    filters: [
                        { dataSize: raydium_sdk_1.MARKET_STATE_LAYOUT_V3.span },
                        {
                            memcmp: {
                                offset: raydium_sdk_1.MARKET_STATE_LAYOUT_V3.offsetOf("quoteMint"),
                                bytes: baseMint.toBase58(),
                            },
                        },
                        {
                            memcmp: {
                                offset: raydium_sdk_1.MARKET_STATE_LAYOUT_V3.offsetOf("baseMint"),
                                bytes: quoteMint.toBase58(),
                            },
                        },
                    ],
                });
            console.log(accounts);
            return accounts.map(({ account }) => raydium_sdk_1.MARKET_STATE_LAYOUT_V3.decode(account.data))[0].ownAddress;
        });
    }
    static fetchMarketInfo(connection, marketId) {
        return __awaiter(this, void 0, void 0, function* () {
            const marketAccountInfo = yield connection.getAccountInfo(marketId, "processed");
            if (!marketAccountInfo) {
                throw new Error('Failed to fetch market info for market id ' + marketId.toBase58());
            }
            return raydium_sdk_1.MARKET_STATE_LAYOUT_V3.decode(marketAccountInfo.data);
        });
    }
    static generateV4PoolInfo(baseMint, quoteMint, marketID) {
        return __awaiter(this, void 0, void 0, function* () {
            const poolInfo = raydium_sdk_1.Liquidity.getAssociatedPoolKeys({
                version: 4,
                marketVersion: 3,
                baseMint: baseMint,
                quoteMint: quoteMint,
                baseDecimals: 0,
                quoteDecimals: this.SOL_DECIMALS,
                programId: new web3_js_1.PublicKey('675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8'),
                marketId: marketID,
                marketProgramId: new web3_js_1.PublicKey('srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX'),
            });
            return { poolInfo };
        });
    }
    static fetchPoolKeyInfo(connection, baseMint, quoteMint) {
        return __awaiter(this, void 0, void 0, function* () {
            const marketId = yield this.fetchMarketId(connection, baseMint, quoteMint, 'confirmed');
            // const marketInfo = await this.fetchMarketInfo(connection, marketId);
            // const baseMintInfo = await connection.getParsedAccountInfo(baseMint, "confirmed") as MintInfo;
            // const baseDecimals = baseMintInfo.value.data.parsed.info.decimals
            const V4PoolInfo = yield this.generateV4PoolInfo(baseMint, quoteMint, marketId);
            const lpMintInfo = yield connection.getParsedAccountInfo(V4PoolInfo.poolInfo.lpMint, "confirmed");
            return {
                id: V4PoolInfo.poolInfo.id,
                marketId: marketId,
                baseMint: baseMint,
                quoteMint: quoteMint,
                baseVault: V4PoolInfo.poolInfo.baseVault,
                quoteVault: V4PoolInfo.poolInfo.quoteVault,
                lpMint: V4PoolInfo.poolInfo.lpMint,
                // baseDecimals: baseDecimals,
                quoteDecimals: this.SOL_DECIMALS,
                lpDecimals: lpMintInfo.value.data.parsed.info.decimals,
                version: 4,
                programId: new web3_js_1.PublicKey(this.RAYDIUM_POOL_V4_PROGRAM_ID),
                authority: V4PoolInfo.poolInfo.authority,
                openOrders: V4PoolInfo.poolInfo.openOrders,
                targetOrders: V4PoolInfo.poolInfo.targetOrders,
                withdrawQueue: new web3_js_1.PublicKey("11111111111111111111111111111111"),
                lpVault: new web3_js_1.PublicKey("11111111111111111111111111111111"),
                marketVersion: 3,
                marketProgramId: new web3_js_1.PublicKey(this.OPENBOOK_ADDRESS),
                marketAuthority: raydium_sdk_1.Market.getAssociatedAuthority({ programId: new web3_js_1.PublicKey(this.OPENBOOK_ADDRESS), marketId: marketId }).publicKey,
                // marketBaseVault: marketInfo.baseVault,
                // marketQuoteVault: marketInfo.quoteVault,
                // marketBids: marketInfo.bids,
                // marketAsks: marketInfo.asks,
                // marketEventQueue: marketInfo.eventQueue,
                lookupTableAccount: web3_js_1.PublicKey.default
            };
        });
    }
}
exports.PoolKeys = PoolKeys;
PoolKeys.SOLANA_ADDRESS = 'So11111111111111111111111111111111111111112';
PoolKeys.RAYDIUM_POOL_V4_PROGRAM_ID = '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8';
PoolKeys.OPENBOOK_ADDRESS = 'srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX';
PoolKeys.SOL_DECIMALS = 9;
