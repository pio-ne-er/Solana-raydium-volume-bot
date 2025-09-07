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
exports.solanaConnection = void 0;
const web3_js_1 = require("@solana/web3.js");
const constants_1 = require("./constants");
const utils_1 = require("./utils");
const bs58_1 = __importDefault(require("bs58"));
exports.solanaConnection = new web3_js_1.Connection(constants_1.RPC_ENDPOINT, {
    wsEndpoint: constants_1.RPC_WEBSOCKET_ENDPOINT,
});
const mainKp = web3_js_1.Keypair.fromSecretKey(bs58_1.default.decode(constants_1.PRIVATE_KEY));
const gather = () => __awaiter(void 0, void 0, void 0, function* () {
    const data = (0, utils_1.readJson)();
    if (data.length == 0) {
        console.log("No wallet to gather");
        return;
    }
    for (let i = 0; i < data.length; i++) {
        try {
            const wallet = web3_js_1.Keypair.fromSecretKey(bs58_1.default.decode(data[i].privateKey));
            const balance = yield exports.solanaConnection.getBalance(wallet.publicKey);
            if (balance == 0) {
                console.log("sol balance is 0, skip this wallet");
                continue;
            }
            const rent = yield exports.solanaConnection.getMinimumBalanceForRentExemption(32);
            console.log("🚀 ~ gather ~ minBalance:", rent);
            const transaction = new web3_js_1.Transaction().add(web3_js_1.ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 600000 }), web3_js_1.ComputeBudgetProgram.setComputeUnitLimit({ units: 20000 }), web3_js_1.SystemProgram.transfer({
                fromPubkey: wallet.publicKey,
                toPubkey: mainKp.publicKey,
                lamports: balance - 13 * 10 ** 3 - rent
            }));
            transaction.recentBlockhash = (yield exports.solanaConnection.getLatestBlockhash()).blockhash;
            transaction.feePayer = wallet.publicKey;
            console.log(yield exports.solanaConnection.simulateTransaction(transaction));
            const sig = yield (0, web3_js_1.sendAndConfirmTransaction)(exports.solanaConnection, transaction, [wallet], { skipPreflight: true });
            console.log({ sig });
        }
        catch (error) {
            console.log("Failed to gather sol in a wallet");
        }
    }
});
gather();
