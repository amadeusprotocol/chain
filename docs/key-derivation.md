# Amadeus key derivation

Status: normative for every Amadeus signer (browser extension, phone app,
hub.ama.one, amahub.ama.one, SDKs). Decided 2026-10-11 (vans163: "everything
should use the standard hdwallet and eip2333").

## 1. The rule

One recovery phrase MUST resolve to one Amadeus account in every signer.

```
phrase (BIP39, 12 or 24 English words, NFKD, lower case, single spaces)
  → seed      = PBKDF2-HMAC-SHA512(phrase, "mnemonic" + passphrase, 2048 rounds, 64 bytes)
                passphrase is always the empty string
  → master    = EIP-2333 derive_master_SK(seed)
  → child     = EIP-2333 derive_child_SK along m / 12381 / 36969 / account / 0 / 0
  → sk        = child as a 32-byte big-endian integer, 0 < sk < r
  → pk        = sk · G1, 48-byte compressed, shown base58
```

| Level     | Value   | Why                                                              |
| --------- | ------- | ---------------------------------------------------------------- |
| purpose   | 12381   | EIP-2334 purpose for BLS12-381 keys                              |
| coin type | 36969   | Amadeus. In production on hub.ama.one and amahub.ama.one since 2026-07 |
| account   | 0, 1, … | 0 is the wallet a phrase shows first                              |
| use       | 0       | EIP-2334 signing key                                              |
| index     | 0       | reserved, always 0                                               |

`r = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001`
(the BLS12-381 scalar field order). Signatures are unchanged: G2 point,
DST as today. Only the phrase → sk step is specified here.

Changing the coin type, the path depth or the seed passphrase forks every
existing Google / social-login identity. Do not.

## 2. Reference implementation

`@amadeus-protocol/sdk`, module `src/hd.ts`:

- `mnemonicToHdKeypair(phrase, path = AMADEUS_HD_PATH)` → `{ publicKey, privateKey }`
- `mnemonicToHdSeedBase58(phrase)` → the 64-byte seed the SDK vault, `TransactionBuilder`
  and `signMessage` already accept (the 32-byte scalar written little-endian into the
  first 32 bytes, rest zero; `reduce512To256LE` returns the scalar unchanged)
- `deriveMasterSK`, `deriveChildSK`, `deriveChildSKMultiple` for other paths

Any other implementation MUST reproduce the vectors in section 4.

## 3. Legacy derivation and import

Until 2026-10 the browser extension (SDK `mnemonicToSeedBase58` +
`reduce512To256LE`) derived

```
sk_legacy = int.from_bytes(seed, "little") mod r
```

with no HKDF and no path. Phrases created by those extension versions still
exist and hold funds and Prime. Therefore:

1. A **new** phrase is always derived per section 1. Never create a legacy key.
2. On **import** of a phrase, derive both `pk_hd` and `pk_legacy`
   (`mnemonicToCandidateKeypairs` in the SDK) and look each up: chain history
   (any tx or balance), Prime balance, hub account.
   - exactly one has history → open that one, silently
   - neither has history → open `pk_hd`
   - both have history → let the user choose, show both addresses
3. A legacy account keeps working; the signer stores which derivation a vault
   uses. The extension's existing vault format `{ seed, mnemonic }` needs no
   change, because both derivations produce a 64-byte seed that the signing
   path consumes as today.
4. Signers MUST NOT show a legacy-derived empty address for a phrase whose
   HD address has history. That is the defect this document closes.

## 4. Test vectors

EIP-2333 official vectors (master and one child) are in the SDK test
`src/__tests__/hd.test.ts` and in https://eips.ethereum.org/EIPS/eip-2333#test-cases.

Full Amadeus path, phrase
`abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about`
(the standard BIP39 test phrase; never fund it):

| Derivation                       | Secret key (hex, big-endian)                                       |
| -------------------------------- | ------------------------------------------------------------------ |
| m/12381/36969/0/0/0 (standard)   | `49fe42277a95f876891e7f70d4673eabbab099d6374b051ae9023ad36bdc0bb7` |
| legacy seed mod r (import only)  | `73e399517309113e929253a7163caf4d8fada5b1606f36f6ce6c3fd7a199c2a3` |

Legacy public key for that phrase, base58:
`72rGNmodqaeA1K7eiJWHbodatk4gCzXur4gSoYTN1go9Pr2SYcdyX1o4hpeuWmFxCz`.

The standard value was produced by two independent implementations
(amahub.ama.one's `eip2333.js`, which is byte-identical to
`@chainsafe/bls-keygen` as used by hub.ama.one, and the SDK module above).

## 5. Who implements what

| Signer                 | State on 2026-10-11                    | Change                                     |
| ---------------------- | -------------------------------------- | ------------------------------------------ |
| hub.ama.one Google     | standard                               | none                                       |
| amahub.ama.one Google  | standard                               | none                                       |
| TypeScript SDK         | legacy only                            | `src/hd.ts` (this change)                  |
| Browser extension      | legacy                                 | new phrases standard; import per section 3 |
| Phone app              | legacy (assumed, same SDK)             | same as the extension                      |
| Any new signer         | —                                      | section 1 from day one                     |
