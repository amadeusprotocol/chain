# Merkle ZK Proof

Zero-knowledge proof that a transaction exists in a Merkle tree. Verified on zkVerify testnet.

## Usage

```bash
npm install
npm run proof
npm run verify
node scripts/wait_for_tx.js <job-id>
```

## Proof Size

807 bytes

## Examples

- [0xadbf74b95d4c817a...](https://zkverify-testnet.subscan.io/extrinsic/0xadbf74b95d4c817a392f9a6db140a3fc1846df9db82c444c59ed080fa8620252)
- [0x3f99692947c2d866...](https://zkverify-testnet.subscan.io/extrinsic/0x3f99692947c2d86673eaaa64d87ca149770d97299e2c4efa8405a0a91c7924ed)

## Tech

Poseidon hash, 1,972 constraints, Groth16, zkVerify API

API key in `.env`
