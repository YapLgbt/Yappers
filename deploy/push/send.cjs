#!/usr/bin/env node
// The home has already authenticated the installation and checked current visibility.
'use strict';
const fs = require('node:fs');
const crypto = require('node:crypto');
const http2 = require('node:http2');
const webPush = require('web-push');

const env = process.env;
const deadline = 12000;
const seconds = () => Math.floor(Date.now() / 1000);
const enc = object => Buffer.from(JSON.stringify(object)).toString('base64url');
function jwt(header, claims, key) {
  const content = enc(header) + '.' + enc(claims);
  const signature = crypto.sign('sha256',Buffer.from(content),{key,dsaEncoding:'ieee-p1363'});
  return content + '.' + signature.toString('base64url');
}
let oauth;
async function firebaseCredentials() {
  const account=JSON.parse(fs.readFileSync(env.YAP_FCM_CREDENTIALS,'utf8'));
  const address='https://oauth2.googleapis.com/token';
  const iat=seconds();
  const assertion=jwt({alg:'RS256',typ:'JWT'},{iss:account.client_email,
    scope:'https://www.googleapis.com/auth/firebase.messaging',aud:address,iat,exp:iat+3600},account.private_key);
  const response=await fetch(address,{method:'POST',signal:AbortSignal.timeout(deadline),
    body:new URLSearchParams({grant_type:'urn:ietf:params:oauth:grant-type:jwt-bearer',assertion})});
  if (!response.ok) throw Error('FCM authorization failed');
  const {access_token}=await response.json();
  return {project:account.project_id,token:access_token};
}
const generic = 'You have an Inbox update.';
async function send(job) {
  const {route,provider,subscription}=job;
  const revision=String(job.seq);
  if (!/^[a-f0-9]{64}$/.test(route)) throw Error('invalid route');
  if (provider==='web') {
    const data=JSON.stringify({route,revision});
    try {
      await webPush.sendNotification(subscription,data,{
        TTL:86400,topic:route.slice(0,32),timeout:deadline,urgency:'normal',
        vapidDetails:{subject:'https://yap.lgbt',publicKey:env.YAP_WEB_PUSH_KEY,
          privateKey:fs.readFileSync(env.YAP_WEB_PUSH_PRIVATE_FILE,'utf8').trim()}
      });
      return 'ok';
    } catch (e) {return [404,410].includes(e.statusCode)?'gone':'retry';}
  }
  if (provider==='fcm') {
    oauth ??= firebaseCredentials();
    const {project,token}=await oauth;
    const result=await fetch('https://fcm.googleapis.com/v1/projects/'+encodeURIComponent(project)+'/messages:send',{
      method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},
      signal:AbortSignal.timeout(deadline),body:JSON.stringify({message:{token:subscription,data:{route,revision},
        android:{priority:'high',ttl:'86400s',collapse_key:route}}})});
    if (result.ok) return 'ok';
    const error=await result.json();
    return error.error?.details?.some(d=>d.errorCode==='UNREGISTERED')?'gone':'retry';
  }
  if (provider==='apns') {
    const auth=jwt({alg:'ES256',kid:env.YAP_APNS_KEY_ID},{iss:env.YAP_APPLE_TEAM_ID,iat:seconds()},fs.readFileSync(env.YAP_APNS_KEY_FILE));
    const client=http2.connect(env.YAP_APNS_SANDBOX==='1'?'https://api.sandbox.push.apple.com':'https://api.push.apple.com');
    try {
      return await new Promise((resolve,reject)=>{
        client.once('error',reject);
        const req=client.request({':method':'POST',':path':'/3/device/'+subscription,
          authorization:'bearer '+auth,'apns-topic':env.YAP_APNS_TOPIC||'app.yappers.prototype',
          'apns-push-type':'alert','apns-priority':'10','apns-collapse-id':route,'apns-expiration':String(seconds()+86400)});
        let status, body='';
        req.setTimeout(deadline,()=>req.destroy(Error('APNs timeout')));
        req.on('error',reject);
        req.on('response',headers=>{status=headers[':status'];});
        req.on('data',chunk=>{body+=chunk;});
        req.on('end',()=>resolve(status===200?'ok':status===410 ||
          (status===400 && body.includes('BadDeviceToken'))?'gone':'retry'));
        req.end(JSON.stringify({aps:{alert:{title:'Yap',body:generic},'thread-id':route},route,revision}));
      });
    } finally {client.destroy();}
  }
  throw Error('unknown provider');
}
async function main() {
  const jobs=JSON.parse(fs.readFileSync(0,'utf8'));
  const outcomes=await Promise.all(jobs.map(async job=>{
    try {return [job.route,await send(job)];} catch {return [job.route,'retry'];}
  }));
  process.stdout.write(JSON.stringify(Object.fromEntries(outcomes)));
}
main().catch(()=>{process.exitCode=1;});
