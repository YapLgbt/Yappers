import init, { portable_account } from './runtime/yap.js';

self.onmessage = async ({ data }) => {
  try {
    await init();
    self.postMessage({ result: JSON.parse(portable_account(JSON.stringify(data))) });
  } catch {
    self.postMessage({
      error:
        data.type === 'decrypt'
          ? 'Couldn’t open this backup. Check the file and passphrase.'
          : 'Couldn’t prepare this backup. Check the passphrase and available device memory.',
    });
  }
};
