"use strict";
// Jito Bundling part
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
exports.onBundleResult = exports.bull_dozer = exports.bundle = void 0;
const web3_js_1 = require("@solana/web3.js");
const constants_1 = require("../constants");
const bs58_1 = __importDefault(require("bs58"));
const searcher_1 = require("jito-ts/dist/sdk/block-engine/searcher");
const types_1 = require("jito-ts/dist/sdk/block-engine/types");
const utils_1 = require("jito-ts/dist/sdk/block-engine/utils");
const solanaConnection = new web3_js_1.Connection(constants_1.RPC_ENDPOINT, {
    wsEndpoint: constants_1.RPC_WEBSOCKET_ENDPOINT,
});
function bundle(txs, keypair) {
    return __awaiter(this, void 0, void 0, function* () {
        try {
            const txNum = Math.ceil(txs.length / 3);
            let successNum = 0;
            for (let i = 0; i < txNum; i++) {
                const upperIndex = (i + 1) * 3;
                const downIndex = i * 3;
                const newTxs = [];
                for (let j = downIndex; j < upperIndex; j++) {
                    if (txs[j])
                        newTxs.push(txs[j]);
                }
                let success = yield bull_dozer(newTxs, keypair);
                return success;
            }
            if (successNum == txNum)
                return true;
            else
                return false;
        }
        catch (error) {
            return false;
        }
    });
}
exports.bundle = bundle;
function bull_dozer(txs, keypair) {
    return __awaiter(this, void 0, void 0, function* () {
        try {
            const bundleTransactionLimit = parseInt('4');
            const jitoKey = web3_js_1.Keypair.fromSecretKey(bs58_1.default.decode(constants_1.JITO_KEY));
            const search = (0, searcher_1.searcherClient)(constants_1.BLOCKENGINE_URL, jitoKey);
            yield build_bundle(search, bundleTransactionLimit, txs, keypair);
            const bundle_result = yield (0, exports.onBundleResult)(search);
            return bundle_result;
        }
        catch (error) {
            return 0;
        }
    });
}
exports.bull_dozer = bull_dozer;
function build_bundle(search, bundleTransactionLimit, txs, keypair) {
    return __awaiter(this, void 0, void 0, function* () {
        const accounts = yield search.getTipAccounts();
        const _tipAccount = accounts[Math.min(Math.floor(Math.random() * accounts.length), 3)];
        const tipAccount = new web3_js_1.PublicKey(_tipAccount);
        const bund = new types_1.Bundle([], bundleTransactionLimit);
        const resp = yield solanaConnection.getLatestBlockhash("processed");
        bund.addTransactions(...txs);
        let maybeBundle = bund.addTipTx(keypair, constants_1.JITO_FEE, tipAccount, resp.blockhash);
        if ((0, utils_1.isError)(maybeBundle)) {
            throw maybeBundle;
        }
        try {
            yield search.sendBundle(maybeBundle);
        }
        catch (e) { }
        return maybeBundle;
    });
}
const onBundleResult = (c) => {
    let first = 0;
    let isResolved = false;
    return new Promise((resolve) => {
        // Set a timeout to reject the promise if no bundle is accepted within 5 seconds
        setTimeout(() => {
            resolve(first);
            isResolved = true;
        }, 30000);
        c.onBundleResult((result) => {
            if (isResolved)
                return first;
            // clearTimeout(timeout) // Clear the timeout if a bundle is accepted
            const isAccepted = result.accepted;
            const isRejected = result.rejected;
            if (isResolved == false) {
                if (isAccepted) {
                    // console.log(`bundle accepted, ID: ${result.bundleId}  | Slot: ${result.accepted!.slot}`)
                    first += 1;
                    isResolved = true;
                    resolve(first); // Resolve with 'first' when a bundle is accepted
                }
                if (isRejected) {
                    // Do not resolve or reject the promise here
                }
            }
        }, (e) => {
            // Do not reject the promise here
        });
    });
};
exports.onBundleResult = onBundleResult;
