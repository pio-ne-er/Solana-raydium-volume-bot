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
exports.execute = void 0;
const web3_js_1 = require("@solana/web3.js");
const constants_1 = require("../constants");
const execute = (transaction_1, latestBlockhash_1, ...args_1) => __awaiter(void 0, [transaction_1, latestBlockhash_1, ...args_1], void 0, function* (transaction, latestBlockhash, isBuy = true) {
    const solanaConnection = new web3_js_1.Connection(constants_1.RPC_ENDPOINT, {
        wsEndpoint: constants_1.RPC_WEBSOCKET_ENDPOINT,
    });
    const signature = yield solanaConnection.sendRawTransaction(transaction.serialize(), { skipPreflight: true });
    const confirmation = yield solanaConnection.confirmTransaction({
        signature,
        lastValidBlockHeight: latestBlockhash.lastValidBlockHeight,
        blockhash: latestBlockhash.blockhash,
    });
    if (confirmation.value.err) {
        console.log("Confrimtaion error");
        return "";
    }
    else {
        if (isBuy)
            console.log(`Success in buy transaction: https://solscan.io/tx/${signature}`);
        else
            console.log(`Success in Sell transaction: https://solscan.io/tx/${signature}`);
    }
    return signature;
});
exports.execute = execute;
