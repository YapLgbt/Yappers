<script lang="ts">
  import { minuteClock } from './minuteClock';
  import { tick } from 'svelte';
  import ActionButton from './ActionButton.svelte';
  import ContentMenu from './ContentMenu.svelte';
  import AuthorByline from './AuthorByline.svelte';
  import AppIcon from './AppIcon.svelte';
  import { authorLabel, isAnonymous, visible, type Author, type Post, type Profile } from './community';
  let { post, profile, replica, reply, save, hide, block, message, report, remove, vote, mute,preview=false,detail=false,openThread }: {
    detail?:boolean; openThread?:(post:Post,comments:boolean)=>void; preview?:boolean; report?: (kind:"post"|"reply",id:string,text:string)=>void; post: Post; profile: Profile; replica: boolean; reply: (post: Post, trigger: HTMLButtonElement) => void;
    vote:(post:Post,direction:number)=>Promise<void>; mute:(post:Post)=>void;
    save: (post: Post) => void | Promise<void>; hide: (id: string) => void;
    message?: (author: Author) => void; block?: (author: Author) => void; remove: (id: string, label: string,scope?:string) => void;
  } = $props();
  let voting=$state(false);
  async function cast(direction:number){voting=true;try{await vote(post,direction)}finally{voting=false}}
  let expanded = $state(false);
  let replies = $derived(post.replies.filter(item => visible(item, profile)));
  let pending=$derived(profile.outbox.filter(e=>e.payload.action.kind==='reply' && e.payload.action.parent===post.id && !replies.some(r=>r.id===e.id)));
  let comments = $derived<{author: Author; queued: boolean}[]>([
    ...replies.map(author => ({ author, queued: false })),
    ...pending.map(event => ({
      author: {id: event.id, mine: true, author: event.payload.action.displayName === 'Anonymous neighbor' ? 'Anonymous neighbor' : profile.alias,
        profile: event.payload.action.displayName === 'Anonymous neighbor' ? undefined : profile.publicProfile,
        username: event.payload.action.displayName === 'Anonymous neighbor' ? undefined : profile.username,
        body: event.payload.action.body || ''},
      queued: true
    }))
  ]);
  let saved = $derived(profile.saved.includes(post.id));
  let saving = false;
  async function toggleSave(button: HTMLButtonElement) {
    if (saving) return;
    saving = true;
    const wasSaved = saved;
    try {
      await save(post);
      await tick();
      if (!wasSaved && saved && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
        button.querySelector('svg')?.animate(
          [{ transform: 'translateY(-3px)' }, { transform: 'translateY(0)' }],
          { duration: 150, easing: 'ease-out' }
        );
      }
    } finally {
      saving = false;
    }
  }
  let timestamp = $derived(post.acceptedAt || post.createdAt);
  let date = $derived(timestamp ? new Date(timestamp * 1000) : null);
  let ageMinutes = $derived(timestamp ? Math.max(0, Math.floor(($minuteClock / 1000 - timestamp) / 60)) : 0);
  let timeLabel = $derived(ageMinutes < 1 ? 'Just now' : ageMinutes < 60 ? ageMinutes + 'm ago'
    : ageMinutes < 1440 ? Math.floor(ageMinutes / 60) + 'h ago'
    : date?.toLocaleDateString(undefined, {month: 'short', day: 'numeric'}));
  let canMessage = $derived(!!message && !!profile.directAvailable && !replica && !post.mine &&
    !!post.visibility?.peer && !isAnonymous(post));
</script>

<article data-post-id={post.id} data-pending={post.pending || undefined} class:thread-post={detail}>
  <div class="post-head">
    <AuthorByline author={post} message={canMessage ? () => message?.(post) : undefined}/>
    {#if !post.pending}<ContentMenu label="Post options" items={[
      ...(!preview?[{label:'Report post',run:()=>report?.('post',post.id,[post.headline,post.body].filter(Boolean).join('\n'))}]:[]),
      {label:'Mute conversation',run:()=>mute(post)},
      {label:'Hide on this device',run:()=>hide(post.id)},
      ...(!replica && (post.mine||!preview)?[post.mine?{label:'Delete post',run:()=>remove(post.id,'post',post.scope)}:{label:'Block account',run:()=>block?.(post)}]:[])
    ]}/>{/if}
  </div>
  {#if openThread && !detail}
    {#if post.headline}<h2><button class="read-post" id={'read-'+post.id} onclick={()=>openThread?.(post,false)}>{post.headline}</button></h2>{/if}
    <p><button class="read-post" tabindex={post.headline ? -1 : undefined} id={post.headline ? undefined : 'read-'+post.id} onclick={()=>openThread?.(post,false)}>{post.body}</button></p>
  {:else}
    {#if post.headline}<h2>{post.headline}</h2>{/if}<p>{post.body}</p>
  {/if}
  <div class="post-meta">
    <span class="post-topic">{post.topic}</span>{#if post.area}{' · '}{post.area.label}{/if}
    {#if date}{' · '}<time datetime={date.toISOString()} title={date.toLocaleString()}>{timeLabel}</time>{/if}
  </div>
  <div class="post-actions">
    {#if post.pending}<span class="pending-post-label" role="status">Waiting to send</span>{:else}
    <button type="button" id={detail ? undefined : 'comments-'+post.id}
      aria-label={'Replies (' + comments.length + ')'} title="Replies"
      aria-expanded={detail || openThread ? undefined : expanded} onclick={() => {
        if (detail) document.getElementById('thread-comments')?.focus();
        else if (openThread) openThread(post, true);
        else expanded = !expanded;
      }}><span class="reply-count"><AppIcon name="conversation" />{comments.length}</span></button>
    {#if !replica}
      <div class="vote-pair">
        <button type="button" aria-label="Upvote" title="Upvote" aria-pressed={post.vote === 1} disabled={voting} onclick={() => cast(1)}><AppIcon name="up" /></button>
        <span aria-label="Score">{post.score || 0}</span>
        <button type="button" aria-label="Downvote" title="Downvote" aria-pressed={post.vote === -1} disabled={voting} onclick={() => cast(-1)}><AppIcon name="down" /></button>
      </div>
    {/if}
    <button type="button" class="bookmark" aria-label={saved ? 'Saved' : 'Save'} title={saved ? 'Saved' : 'Save'} aria-pressed={saved} onclick={event => toggleSave(event.currentTarget)}><AppIcon name="saved" /></button>{/if}
  </div>
  {#if detail || expanded}
    <div data-thread={post.id}>
      <div class="comment-tools">
        <h3 id={detail ? 'thread-comments' : undefined} tabindex="-1">Comments</h3>
        <ActionButton variant="outlined" disabled={post.pending} onclick={event => reply(post, event.currentTarget)}>Write a reply</ActionButton>
      </div>
      {#each comments as {author: item, queued} (item.id)}
        {@const replyMessage = message && profile.directAvailable && !replica && !item.mine && item.visibility?.peer && !isAnonymous(item)
          ? () => message?.({...item, scope:post.scope}) : undefined}
        <div class="reply" tabindex="-1" id={'reply-'+item.id}>
          <div class="post-head">
          <AuthorByline author={item} message={replyMessage}/>
          {#if !item.mine && !replica && !preview}<ContentMenu label={'Reply options for '+authorLabel(item)} items={[
            {label:'Report reply',run:()=>report?.('reply',item.id,item.body)},{label:'Block account',run:()=>block?.({...item,scope:post.scope})}
          ]}/>{/if}
          </div>
          <p>{item.body}</p>
          {#if queued}<small class="reply-delivery" role="status">Waiting to sync</small>{:else if item.mine && !replica}<ActionButton onclick={() => remove(item.id,'reply',post.scope)}>Delete reply</ActionButton>{/if}
        </div>
      {/each}
      {#if !comments.length}<p class="no-comments">No comments yet.</p>{/if}
    </div>
  {/if}
</article>
