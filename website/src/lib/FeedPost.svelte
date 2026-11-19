<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import ContentMenu from './ContentMenu.svelte';
  import AuthorByline from './AuthorByline.svelte';
  import AppIcon from './AppIcon.svelte';
  import { authorLabel, isAnonymous, visible, type Author, type Post, type Profile } from './community';
  let { post, profile, replica, reply, save, hide, block, message, report, remove, vote, mute,preview=false,detail=false,openThread }: {
    detail?:boolean; openThread?:(post:Post,comments:boolean)=>void; preview?:boolean; report?: (kind:"post"|"reply",id:string,text:string)=>void; post: Post; profile: Profile; replica: boolean; reply: (post: Post) => void;
    vote:(post:Post,direction:number)=>Promise<void>; mute:(post:Post)=>void;
    save: (post: Post) => void; hide: (id: string) => void;
    message?: (author: Author) => void; block?: (author: Author) => void; remove: (id: string, label: string,scope?:string) => void;
  } = $props();
  let voting=$state(false);
  async function cast(direction:number){voting=true;try{await vote(post,direction)}finally{voting=false}}
  let expanded = $state(false);
  let replies = $derived(post.replies.filter(item => visible(item, profile)));
  let pending=$derived(profile.outbox.filter(e=>e.payload.action.kind==='reply' && e.payload.action.parent===post.id && !replies.some(r=>r.id===e.id)));
  let saved = $derived(profile.saved.includes(post.id));
  let date = $derived(post.acceptedAt ? new Date(post.acceptedAt * 1000) : null);
  let canMessage = $derived(!!message && !!profile.directAvailable && !replica && !post.mine &&
    !!post.visibility?.peer && !isAnonymous(post));
</script>

<article>
  <div class="post-head">
    <AuthorByline author={post} message={canMessage ? () => message?.(post) : undefined}/>
    <ContentMenu label="Post options" items={[
      ...(!preview?[{label:'Report post',run:()=>report?.('post',post.id,[post.headline,post.body].filter(Boolean).join('\n'))}]:[]),
      {label:'Mute conversation',run:()=>mute(post)},
      {label:'Hide on this device',run:()=>hide(post.id)},
      ...(!replica && (post.mine||!preview)?[post.mine?{label:'Delete post',run:()=>remove(post.id,'post',post.scope)}:{label:'Block account',run:()=>block?.(post)}]:[])
    ]}/>
  </div>
  {#if openThread && !detail}
    {#if post.headline}<h2><button class="read-post" id={'read-'+post.id} onclick={()=>openThread?.(post,false)}>{post.headline}</button></h2>{/if}
    <p><button class="read-post" id={post.headline ? undefined : 'read-'+post.id} onclick={()=>openThread?.(post,false)}>{post.body}</button></p>
  {:else}
    {#if post.headline}<h2>{post.headline}</h2>{/if}<p>{post.body}</p>
  {/if}
  <div class="post-meta">
    <span class="post-topic">{post.topic}</span>{#if post.area}{' · '}{post.area.label}{/if}
    {#if date}{' · '}<time datetime={date.toISOString()} title={date.toLocaleString()}>{date.toLocaleDateString(undefined,{month:'short',day:'numeric'})}</time>{/if}
  </div>
  <div class="tools">
    <ActionButton variant={replies.length || pending.length ? 'tonal' : 'text'} id={detail ? undefined : 'comments-'+post.id} aria-expanded={detail || openThread ? undefined : expanded} onclick={() => {
      if(detail) document.getElementById('thread-comments')?.focus();
      else if(openThread) openThread(post,true); else expanded=!expanded;
    }}><AppIcon name="nearby" />Replies ({replies.length+pending.length})</ActionButton>
    <button class="bookmark mtrl-icon-button mtrl-icon-button--standard" aria-label={saved ? 'Saved' : 'Save'} title={saved ? 'Saved' : 'Save'} aria-pressed={saved} onclick={() => save(post)}><AppIcon name="saved" /></button>
    {#if !replica}<div class="vote-pair"><button class="mtrl-icon-button mtrl-icon-button--standard" aria-label="Upvote" aria-pressed={post.vote===1} disabled={voting} onclick={()=>cast(1)}><AppIcon name="up"/></button><span aria-label="Score">{post.score||0}</span><button class="mtrl-icon-button mtrl-icon-button--standard" aria-label="Downvote" aria-pressed={post.vote===-1} disabled={voting} onclick={()=>cast(-1)}><AppIcon name="down"/></button></div>{/if}
  </div>
  {#if detail || expanded}
    <div data-thread={post.id}>
      {#if detail}<h3 id="thread-comments" tabindex="-1">Comments</h3>{/if}
      {#each replies as item (item.id)}
        {@const replyMessage = message && profile.directAvailable && !replica && !item.mine && item.visibility?.peer && !isAnonymous(item)
          ? () => message?.({...item, scope:post.scope}) : undefined}
        <div class="reply" id={'reply-'+item.id}>
          <div class="post-head">
          <AuthorByline author={item} message={replyMessage}/>
          {#if !item.mine && !replica && !preview}<ContentMenu label={'Reply options for '+authorLabel(item)} items={[
            {label:'Report reply',run:()=>report?.('reply',item.id,item.body)},{label:'Block account',run:()=>block?.({...item,scope:post.scope})}
          ]}/>{/if}
          </div>
          <p>{item.body}</p>
          {#if item.mine && !replica}<ActionButton onclick={() => remove(item.id,'reply',post.scope)}>Delete reply</ActionButton>{/if}
        </div>
      {/each}
      {#each pending as event (event.id)}<div class="reply" id={'reply-'+event.id}><strong>{event.payload.action.displayName}</strong><p>{event.payload.action.body}</p><small>Waiting to sync</small></div>{/each}
      {#if !replies.length && !pending.length}<p>No comments yet.</p>{/if}
      <ActionButton onclick={() => reply(post)}>Write a reply</ActionButton>
    </div>
  {/if}
</article>
