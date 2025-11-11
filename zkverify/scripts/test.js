const { buildPoseidon } = require("circomlibjs");
const snarkjs = require("snarkjs");
const fs = require("fs");

async function generateTestProof() {
    const poseidon = await buildPoseidon();
    const F = poseidon.F;

    const leaves = [];
    for (let i = 0; i < 8; i++) {
        leaves.push(F.toObject(F.e(1000 + i)));
    }

    let level = leaves.map(l => F.toObject(poseidon([l])));
    const tree = [level];

    for (let i = 0; i < 3; i++) {
        const nextLevel = [];
        for (let j = 0; j < level.length; j += 2) {
            const left = level[j];
            const right = level[j + 1] || "0";
            const parent = F.toObject(poseidon([left, right]));
            nextLevel.push(parent);
        }
        tree.push(nextLevel);
        level = nextLevel;
    }

    const root = tree[3][0];
    const leafIndex = 5;
    const leafValue = leaves[leafIndex];

    const siblings = [];
    const indices = [];
    let currentIndex = leafIndex;

    for (let lvl = 0; lvl < 3; lvl++) {
        const isRight = currentIndex % 2 === 1;
        const siblingIndex = isRight ? currentIndex - 1 : currentIndex + 1;
        const sibling = tree[lvl][siblingIndex] || "0";
        siblings.push(sibling.toString());
        indices.push(isRight ? "1" : "0");
        currentIndex = Math.floor(currentIndex / 2);
    }

    const input = {
        root: root.toString(),
        leaf: leafValue.toString(),
        siblings: siblings,
        indices: indices
    };

    fs.mkdirSync("build", { recursive: true });
    fs.writeFileSync("build/input.json", JSON.stringify(input, null, 2));

    await snarkjs.wtns.calculate(input, "build/merkle_js/merkle.wasm", "build/witness.wtns");
    console.log("✓ Test data and witness generated");
}

generateTestProof().catch(console.error);
