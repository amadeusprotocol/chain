const axios = require("axios");
require("dotenv").config();

async function waitForTx(jobId, maxWaitSec = 60) {
    const API_KEY = process.env.API_KEY;
    if (!API_KEY) {
        console.error("❌ API_KEY not found");
        process.exit(1);
    }

    console.log(`Waiting for verification (job: ${jobId})...\n`);

    const startTime = Date.now();
    let attempts = 0;

    while ((Date.now() - startTime) / 1000 < maxWaitSec) {
        attempts++;
        process.stdout.write(`Attempt ${attempts}... `);

        try {
            const response = await axios.get(
                `https://relayer-api-testnet.horizenlabs.io/api/v1/job-status/${API_KEY}/${jobId}`
            );

            const data = response.data;
            const status = data.status || data.state;
            console.log(status);

            if (status === "IncludedInBlock" || data.txHash) {
                console.log(`\n✅ Verified on-chain!`);
                console.log(`TX: ${data.txHash}`);
                console.log(`\nExplorer: https://zkverify-testnet.subscan.io/extrinsic/${data.txHash}`);
                return data.txHash;
            }

            if (status === "Failed" || status === "Error") {
                console.log(`\n❌ Verification failed:`, data.errorDetails || data);
                process.exit(1);
            }

            await new Promise(resolve => setTimeout(resolve, 3000));
        } catch (error) {
            console.log(`Error (${error.response?.status || error.message})`);
            await new Promise(resolve => setTimeout(resolve, 3000));
        }
    }

    console.log(`\n⏱️ Timeout after ${maxWaitSec}s - check manually:`);
    console.log(`node scripts/wait_for_tx.js ${jobId}`);
}

if (process.argv.length < 3) {
    console.log("Usage: node scripts/wait_for_tx.js <job-id> [max-wait-seconds]");
    process.exit(1);
}

waitForTx(process.argv[2], parseInt(process.argv[3]) || 60).catch(console.error);
