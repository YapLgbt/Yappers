// A WebAuthn PRF derives a wrapping key locally. The downloaded file belongs to the user.
const encoder = new TextEncoder();
const decoder = new TextDecoder();
const format = 'yap-passkey-recovery-v1';
const random = n => crypto.getRandomValues(new Uint8Array(n));
const base64 = bytes => btoa(String.fromCharCode(...new Uint8Array(bytes)));
const bytes = text => Uint8Array.from(atob(text), char => char.charCodeAt(0));
const hex = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);

const fileUnavailable = 'Your passkey was created, but it can’t protect a Yap recovery file. Copy your recovery key above and save it in your password manager.';

function requireWebAuthn() {
  if (!window.PublicKeyCredential || !navigator.credentials || !window.isSecureContext) {
    throw new Error('Passkeys are unavailable here. To make a backup, copy your recovery key. To open a passkey file, use a browser where you can access its passkey.');
  }
}
async function wrappingKey(record, result, unavailable = 'This passkey can’t unlock this file here. Try the browser and passkey manager you used to create it.') {
  const id = bytes(record.credential);
  if (!result) {
    const assertion = await navigator.credentials.get({publicKey:{
      challenge:random(32), allowCredentials:[{type:'public-key',id}],
      userVerification:'required', timeout:90000,
      extensions:{prf:{eval:{first:bytes(record.salt)}}}
    }});
    if (!assertion || base64(assertion.rawId) !== record.credential) throw new Error('Choose the passkey used for this file.');
    result = assertion.getClientExtensionResults().prf?.results?.first;
  }
  if (!result || result.byteLength !== 32) throw new Error(unavailable);
  return crypto.subtle.importKey('raw',result,'AES-GCM',false,['encrypt','decrypt']);
}
function header(record) {
  return encoder.encode(JSON.stringify([format,record.origin,record.credential,record.salt]));
}
export async function createRecoveryFile(wallet) {
  requireWebAuthn();
  const account = wallet.chain.at(-1).payload.account;
  const salt = random(32);
  const credential = await navigator.credentials.create({publicKey:{
    challenge:random(32), rp:{name:'Yap'}, user:{id:random(32), name:'Yap '+account.slice(0,8), displayName:'Yap account '+account.slice(0,8)},
    pubKeyCredParams:[{type:'public-key',alg:-7},{type:'public-key',alg:-257}],
    authenticatorSelection:{residentKey:'required',userVerification:'required'}, attestation:'none',
    timeout:90000, extensions:{prf:{eval:{first:salt}}}
  }});
  if (!credential) throw new Error('Passkey creation was cancelled.');
  const extension = credential.getClientExtensionResults().prf;
  if (!extension?.enabled) throw new Error(fileUnavailable);
  const record = {format,origin:location.origin,credential:base64(credential.rawId),salt:base64(salt)};
  const key = await wrappingKey(record,extension.results?.first,fileUnavailable);
  const iv = random(12);
  const payload = encoder.encode(JSON.stringify({account,home:wallet.chain.at(-1).payload.home,root:wallet.rootSecret}));
  const cipher = await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData:header(record)},key,payload);
  return new Blob([JSON.stringify({...record,iv:base64(iv),cipher:base64(cipher)})],{type:'application/json'});
}
export async function readRecoveryFile(file) {
  requireWebAuthn();
  if (!file || file.size > 16384) throw new Error('Choose a Yap recovery file under 16 KB.');
  let record;
  try {
    record = JSON.parse(await file.text());
    if (record.format !== format || record.origin !== location.origin || typeof record.credential !== 'string' ||
        bytes(record.credential).length < 1 || bytes(record.credential).length > 1023 ||
        bytes(record.salt).length !== 32 || bytes(record.iv).length !== 12 || bytes(record.cipher).length < 16) throw Error();
  } catch { throw new Error('Choose a Yap recovery file created on this site.'); }
  const key = await wrappingKey(record);
  let identity;
  try {
    const clear = await crypto.subtle.decrypt({name:'AES-GCM',iv:bytes(record.iv),additionalData:header(record)},key,bytes(record.cipher));
    identity = JSON.parse(decoder.decode(clear));
    if (!hex(identity.account) || !hex(identity.home) || !hex(identity.root)) throw Error();
  } catch { throw new Error('This passkey could not unlock the file. Check the passkey and the original download.'); }
  return identity;
}
