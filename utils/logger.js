"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.logger = void 0;
const pino_1 = __importDefault(require("pino"));
const transport = pino_1.default.transport({
    target: 'pino-pretty',
});
exports.logger = (0, pino_1.default)({
    level: 'info',
    redact: ['poolKeys'],
    serializers: {
        error: pino_1.default.stdSerializers.err,
    },
    base: undefined,
}, transport);
