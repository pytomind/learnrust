const { ethers } = require('e:/2026/ANTIGRAVITY/namedox/server/node_modules/ethers');
const wordlist = ethers.wordlists.en;
const target = '0x55a2d84a26343c6ad7Ab4713f241C07f70dc025C'.toLowerCase();

const prefix = ['issue', 'dizzy', 'host', 'pretty', 'ghost'];
const suffix = ['vital', 'differ', 'pencil', 'pretty', 'benefit'];

console.log('Searching for target wallet:', target);
console.log('Template: issue dizzy host pretty ghost ? ? vital differ pencil pretty benefit');

const t0 = Date.now();
let tested = 0;
let found = null;

for (let i = 0; i < 2048; i++) {
  const w1 = wordlist.getWord(i);
  for (let j = 0; j < 2048; j++) {
    const w2 = wordlist.getWord(j);
    const phrase = [...prefix, w1, w2, ...suffix].join(' ');

    if (!ethers.Mnemonic.isValidMnemonic(phrase)) continue;

    tested++;
    try {
      // Standard Ethereum derivation path m/44'/60'/0'/0/0
      const wallet = ethers.HDNodeWallet.fromMnemonic(
        ethers.Mnemonic.fromPhrase(phrase),
        "m/44'/60'/0'/0/0"
      );
      if (wallet.address.toLowerCase() === target) {
        found = {
          word6: w1,
          word7: w2,
          fullPhrase: phrase,
          address: wallet.address
        };
        console.log('🎉 FOUND EXACT SEED PHRASE!');
        console.log(JSON.stringify(found, null, 2));
        process.exit(0);
      }
    } catch (e) {}
  }

  if (i > 0 && i % 200 === 0) {
    const elapsedSec = ((Date.now() - t0) / 1000).toFixed(1);
    console.log(`Progress: tested ${i}/2048 outer words (${tested} valid BIP-39 phrases in ${elapsedSec}s)...`);
  }
}

if (!found) {
  console.log(`Scan complete. Tested ${tested} valid phrases. Target not found at m/44'/60'/0'/0/0.`);
}

