#!/bin/bash
set -e
mkdir -p build

echo "Compiling circuit..."
npx circom2 circuits/merkle.circom --r1cs --wasm --sym -o build > /dev/null

echo "Generating test data..."
node scripts/test.js > /dev/null

echo "Setting up ceremony..."
if [ ! -f "build/pot12_final.ptau" ]; then
    npx snarkjs powersoftau new bn128 12 build/pot12.ptau > /dev/null
    npx snarkjs powersoftau contribute build/pot12.ptau build/pot12_c.ptau --name="C" -e="r" > /dev/null
    npx snarkjs powersoftau prepare phase2 build/pot12_c.ptau build/pot12_final.ptau > /dev/null
fi

echo "Generating keys..."
npx snarkjs groth16 setup build/merkle.r1cs build/pot12_final.ptau build/merkle.zkey > /dev/null
npx snarkjs zkey contribute build/merkle.zkey build/merkle_final.zkey --name="F" -e="r" > /dev/null
npx snarkjs zkey export verificationkey build/merkle_final.zkey build/verification_key.json > /dev/null

echo "Creating proof..."
npx snarkjs groth16 prove build/merkle_final.zkey build/witness.wtns build/proof.json build/public.json > /dev/null

echo "✅ Proof: $(wc -c < build/proof.json) bytes → build/proof.json"
echo ""
echo "Verify: npm run verify"
