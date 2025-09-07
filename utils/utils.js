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
exports.editJson = exports.writeJson = exports.readJson = exports.deleteConsoleLines = exports.sleep = exports.saveDataToFile = exports.randVal = exports.retrieveEnvVariable = void 0;
const dotenv_1 = __importDefault(require("dotenv"));
const fs_1 = __importDefault(require("fs"));
dotenv_1.default.config();
const retrieveEnvVariable = (variableName, logger) => {
    const variable = process.env[variableName] || '';
    if (!variable) {
        console.log(`${variableName} is not set`);
        process.exit(1);
    }
    return variable;
};
exports.retrieveEnvVariable = retrieveEnvVariable;
const randVal = (min, max, count, total, isEven) => {
    const arr = Array(count).fill(total / count);
    if (isEven)
        return arr;
    if (max * count < total)
        throw new Error("Invalid input: max * count must be greater than or equal to total.");
    if (min * count > total)
        throw new Error("Invalid input: min * count must be less than or equal to total.");
    const average = total / count;
    // Randomize pairs of elements
    for (let i = 0; i < count; i += 2) {
        // Generate a random adjustment within the range
        const adjustment = Math.random() * Math.min(max - average, average - min);
        // Add adjustment to one element and subtract from the other
        arr[i] += adjustment;
        arr[i + 1] -= adjustment;
    }
    // if (count % 2) arr.pop()
    return arr;
};
exports.randVal = randVal;
const saveDataToFile = (newData, filePath = "data.json") => {
    try {
        let existingData = [];
        // Check if the file exists
        if (fs_1.default.existsSync(filePath)) {
            // If the file exists, read its content
            const fileContent = fs_1.default.readFileSync(filePath, 'utf-8');
            existingData = JSON.parse(fileContent);
        }
        // Add the new data to the existing array
        existingData.push(...newData);
        // Write the updated data back to the file
        fs_1.default.writeFileSync(filePath, JSON.stringify(existingData, null, 2));
    }
    catch (error) {
        try {
            if (fs_1.default.existsSync(filePath)) {
                fs_1.default.unlinkSync(filePath);
                console.log(`File ${filePath} deleted and create new file.`);
            }
            fs_1.default.writeFileSync(filePath, JSON.stringify(newData, null, 2));
            console.log("File is saved successfully.");
        }
        catch (error) {
            console.log('Error saving data to JSON file:', error);
        }
    }
};
exports.saveDataToFile = saveDataToFile;
const sleep = (ms) => __awaiter(void 0, void 0, void 0, function* () {
    yield new Promise((resolve) => setTimeout(resolve, ms));
});
exports.sleep = sleep;
function deleteConsoleLines(numLines) {
    for (let i = 0; i < numLines; i++) {
        process.stdout.moveCursor(0, -1); // Move cursor up one line
        process.stdout.clearLine(-1); // Clear the line
    }
}
exports.deleteConsoleLines = deleteConsoleLines;
// Function to read JSON file
function readJson(filename = "data.json") {
    if (!fs_1.default.existsSync(filename)) {
        // If the file does not exist, create an empty array
        fs_1.default.writeFileSync(filename, '[]', 'utf-8');
    }
    const data = fs_1.default.readFileSync(filename, 'utf-8');
    return JSON.parse(data);
}
exports.readJson = readJson;
// Function to write JSON file
function writeJson(data, filename = "data.json") {
    fs_1.default.writeFileSync(filename, JSON.stringify(data, null, 4), 'utf-8');
}
exports.writeJson = writeJson;
// Function to edit JSON file content
function editJson(newData, filename = "data.json") {
    if (!newData.pubkey) {
        console.log("Pubkey is not prvided as an argument");
        return;
    }
    const wallets = readJson(filename);
    const index = wallets.findIndex(wallet => wallet.pubkey === newData.pubkey);
    if (index !== -1) {
        wallets[index] = Object.assign(Object.assign({}, wallets[index]), newData);
        writeJson(wallets, filename);
    }
    else {
        console.error(`Pubkey ${newData.pubkey} does not exist.`);
    }
}
exports.editJson = editJson;
