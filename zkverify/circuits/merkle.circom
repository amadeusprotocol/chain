pragma circom 2.0.0;

include "../node_modules/circomlib/circuits/poseidon.circom";
include "../node_modules/circomlib/circuits/mux1.circom";

template MerkleProof() {
    signal input root;
    signal input leaf;
    signal input siblings[3];
    signal input indices[3];

    component leafHash = Poseidon(1);
    leafHash.inputs[0] <== leaf;

    component hash0 = Poseidon(2);
    component sel0[2];
    for (var i = 0; i < 2; i++) {
        sel0[i] = Mux1();
    }
    sel0[0].c[0] <== leafHash.out;
    sel0[0].c[1] <== siblings[0];
    sel0[0].s <== indices[0];
    sel0[1].c[0] <== siblings[0];
    sel0[1].c[1] <== leafHash.out;
    sel0[1].s <== indices[0];
    hash0.inputs[0] <== sel0[0].out;
    hash0.inputs[1] <== sel0[1].out;

    component hash1 = Poseidon(2);
    component sel1[2];
    for (var i = 0; i < 2; i++) {
        sel1[i] = Mux1();
    }
    sel1[0].c[0] <== hash0.out;
    sel1[0].c[1] <== siblings[1];
    sel1[0].s <== indices[1];
    sel1[1].c[0] <== siblings[1];
    sel1[1].c[1] <== hash0.out;
    sel1[1].s <== indices[1];
    hash1.inputs[0] <== sel1[0].out;
    hash1.inputs[1] <== sel1[1].out;

    component hash2 = Poseidon(2);
    component sel2[2];
    for (var i = 0; i < 2; i++) {
        sel2[i] = Mux1();
    }
    sel2[0].c[0] <== hash1.out;
    sel2[0].c[1] <== siblings[2];
    sel2[0].s <== indices[2];
    sel2[1].c[0] <== siblings[2];
    sel2[1].c[1] <== hash1.out;
    sel2[1].s <== indices[2];
    hash2.inputs[0] <== sel2[0].out;
    hash2.inputs[1] <== sel2[1].out;

    root === hash2.out;
}

component main {public [root, leaf]} = MerkleProof();
