export interface Author {
  scope?: string;
  id: string;
  author?: string;
  username?: string;
  mine?: boolean;
  visibility?: { anonymous?: boolean; peer?: string; age?:number };
  body: string;
  profile?: PublicProfile;
}
export interface Post extends Author {
  topic: string;
  headline?: string;
  acceptedAt?: number;
  score?: number; vote?:number; area?:Area;
  replies: Author[];
}
export interface BlockedAccount { scope?:string; event: string; label: string; peer?: string; handle?: string; byAccount?: boolean }
type AuthoredEvent = {id:string;payload:{action:{kind:string;headline?:string;body?:string}}};
export interface Profile {
  pendingApprovals?:string[]; pendingPrivacy?:Privacy|null; groupLocality?:Locality|null;
  ownedEvents?:{event:AuthoredEvent;deletion?:unknown}[];recoveredDrafts?:AuthoredEvent[];pendingTransfer?:unknown;
  directAvailable?: boolean;
  mailboxElsewhere?: boolean;
  directState?: { messages: DirectMessage[]; unreadable: number; delivery_paused: boolean };
  messageRequests?: string[];
  approvedContacts?: string[];
  revokedContacts?: string[];
  inboxPolicy?: Privacy;
  nearby?:Nearby|null; activeGroup?:string|null;
  recoveredGroups?:Group[];groupDirectory?:Group[]; joinedGroups?:Record<string,{group:Group}>;
  publicProfile?:PublicProfile; muted?:Record<string,string>;
  join:{community:string};
  alias: string;
  username?: string;
  posts: Post[];
  saved: string[];savedPosts?:Record<string,Post>;
  hiddenPosts?: string[];
  hideAnonymous: boolean;
  blocks: BlockedAccount[];
  pendingUnblocks: string[];
  lastSync?: number;
  postAnonymously?: boolean;
  setupStep?: string | null;
  outbox: { id:string; payload: { community:string;action: { kind: string; target?:string; parent?:string; displayName?:string; value?:number; headline?: string; body?: string } } }[];
}
export type View = 'connecting' | 'offline' | 'primary' | 'replica';
export type ReportSubject = 'post'|'reply'|'message'|'account';
export interface ReportEvidence {subject: ReportSubject; category: string; text:string; reference:string|null}
export interface Community {
  exportArchive(passphrase:string,includeHome:boolean):Promise<Blob>;
  importArchive(file:File,passphrase:string,invitation:string,drafts:boolean):Promise<void>;
  discardUploads(ids:string[]):Promise<void>;
  resetLocalData():Promise<void>;
  reportDraft():Promise<{home:string;receipt:string}>;
  reportStatus(receipt:string):Promise<{status:string}>;
  submitReport(receipt:string,evidence:ReportEvidence):Promise<{status:string}>;
  profile: Profile | null;
  replicaFeed?: { posts: Post[]; asOf: number };
  updates: EventTarget;
  start(intent?: 'create', alias?: string,invitation?:string): Promise<boolean>;
  sync(): Promise<'primary' | 'replica'>;
  modify(change: (profile: Profile) => Profile): Promise<void>;
  queue(action: Record<string, unknown>, alias?: string | null, scope?:string): Promise<string>;
  listGroups():Promise<void>; joinGroup(id:string|null,details?:GroupDetails):Promise<string>;selectGroup(id:string|null):Promise<void>;
  setNearby(value:Nearby|null):Promise<void>;setPrivacy(change:Partial<Privacy>):Promise<void>;
  inboxError: string;
  messageContact(event: string,scope?:string): Promise<Contact>;
  sendMessage(contact: Contact, body: string): Promise<void>;
  unblockAccount(account:BlockedAccount):Promise<void>;
  blockAccount(block: BlockedAccount): Promise<void>;
  approveMessage(peer: string): Promise<void>;
  claimUsername(name: string): Promise<void>;
  verifyWelcomeUsername(finish?:boolean):Promise<{domain:string;username:string|null}>;
  otherMessagingDevice:string;
  usernameDomain(profile: Profile | null): string | null;
  canShowPublic(profile: Profile): boolean;
}

// Browser-only modules share one wallet and one WASM client, including recovery.
export function loadCommunity(): Promise<Community> {
  const url = '/app/network.js';
  return import(/* @vite-ignore */ url);
}
export type RecoveryMode = 'backup' | 'restore';
export async function loadRecovery(dialog: HTMLDialogElement): Promise<(mode: RecoveryMode) => void> {
  const url = '/app/recovery.js';
  const { bindRecovery } = await import(/* @vite-ignore */ url);
  return bindRecovery(dialog);
}

export const topics = ['Chatter', 'Friends', 'Missed connections', 'Plans'];
export const isAnonymous = (author: Author) => author.visibility?.anonymous ??
  (!(author.author || author.username) || author.author === 'Anonymous neighbor');
export const authorLabel = (author: Author) => isAnonymous(author) ? 'Anonymous neighbor' : author.author || author.username || 'Neighbor';
export function visible(author: Author, profile: Profile) {
  if (author.mine) return true;
  if (profile.blocks.some(block => block.event === author.id || (block.peer && block.peer === author.visibility?.peer))) return false;
  return (!profile.hideAnonymous || !isAnonymous(author)) &&
    (author.visibility?.age ?? 0) >= (profile.inboxPolicy?.feedDays ?? 0);
}
export const errorMessage = (error: unknown) => error instanceof Error ? error.message : 'Please try again.';

export interface Contact { peer: string; label: string }
export interface DirectMessage extends Contact {
  id: string;
  body: string;
  outgoing: boolean;
  age: number;
  status: 'received' | 'sent' | 'queued' | 'unavailable';
}
export function inboxFor(profile: Profile) {
  const blocked = new Set(profile.blocks.map(block => block.peer));
  const messages = (profile.directState?.messages || []).filter(message => !blocked.has(message.peer));
  const approved = new Set(profile.approvedContacts);
  const known = new Set(messages.filter(message => message.outgoing && !profile.revokedContacts?.includes(message.peer)).map(message => message.peer));
  const requests = new Set((profile.messageRequests || []).filter(peer => !blocked.has(peer)));
  for (const message of messages) {
    if (!message.outgoing && !known.has(message.peer) && !approved.has(message.peer) &&
      (profile.inboxPolicy?.approvedOnly || message.age < (profile.inboxPolicy?.dmDays || 0))) requests.add(message.peer);
  }
  const threads = new Map<string, DirectMessage>();
  for (const message of messages) {
    if (!requests.has(message.peer)) {
      threads.delete(message.peer);
      threads.set(message.peer, message);
    }
  }
  return {messages, requests:[...requests], threads:[...threads.values()].reverse()};
}

export interface Area { label:string; lat100:number; lon100:number }
export interface Nearby { center:Area; radiusMiles:number }
export interface Locality {city:string; region:string; country:string}
export interface Group { id:string; name:string; category:string; description:string; locality?:Locality | null; homes:number }
export interface GroupDetails {name:string;description:string;category:string;locality:Locality | null}
export interface PublicProfile {pronouns:string;bio:string;identity:string;interests:string;hereFor:string[];photo:string}
export const blankProfile = ():PublicProfile => ({pronouns:'',bio:'',identity:'',interests:'',hereFor:[],photo:''});
export interface Privacy { hideAnonymous:boolean;feedDays:number;dmDays:number;approvedOnly:boolean }
