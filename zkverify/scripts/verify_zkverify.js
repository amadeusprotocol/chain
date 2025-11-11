const axios = require("axios");
const fs = require("fs");
require("dotenv").config();

async function submitProof(proofPath, publicPath, vkeyPath) {
    const API_KEY = process.env.API_KEY;
    if (!API_KEY) {
        console.error("❌ API_KEY not found in .env");
        process.exit(1);
    }

    const proof = JSON.parse(fs.readFileSync(proofPath));
    const publicSignals = JSON.parse(fs.readFileSync(publicPath));
    const verificationKey = JSON.parse(fs.readFileSync(vkeyPath));

    console.log("Submitting proof to zkVerify...");

    try {
        const response = await axios.post(
            `https://relayer-api-testnet.horizenlabs.io/api/v1/submit-proof/${API_KEY}`,
            {
                proofType: "groth16",
                vkRegistered: false,
                proofOptions: { library: "snarkjs", curve: "bn128" },
                proofData: { proof, publicSignals, vk: verificationKey }
            },
            { headers: { "Content-Type": "application/json" } }
        );

        const jobId = response.data.jobId || response.data.id;
        console.log(`✅ Submitted - Job ID: ${jobId}`);

        if (response.data.txHash) {
            console.log(`TX: https://zkverify-testnet.subscan.io/extrinsic/${response.data.txHash}`);
        } else {
            console.log(`Check status: node scripts/wait_for_tx.js ${jobId}`);
        }

        return response.data;
    } catch (error) {
        console.error("❌ Failed:", error.response?.data || error.message);
        process.exit(1);
    }
}

if (process.argv.length < 4) {
    console.log("Usage: node scripts/verify_zkverify.js <proof.json> <public.json>");
    process.exit(1);
}

submitProof(
    process.argv[2],
    process.argv[3],
    process.argv[4] || "build/verification_key.json"
).catch(console.error);
