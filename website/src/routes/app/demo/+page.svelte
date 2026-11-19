<script lang="ts">
  import ActionButton from '../../../lib/ActionButton.svelte';
  import { onMount,tick } from 'svelte';
  import {page} from '$app/state';
  import {pushState,replaceState} from '$app/navigation';
  import { Tabs } from 'bits-ui';
  import AppHead from '../../../lib/AppHead.svelte';
  import FeedPost from '../../../lib/FeedPost.svelte';
  import Sheet from '../../../lib/Sheet.svelte';
  import type { Post, Profile } from '../../../lib/community';
  let ready=$state(false);
  onMount(()=>{ready=true});
  const samples: Post[] = [
    {
      id: 'books',
      author: 'Soft landing',
      headline: 'A book club for the quietly social',
      topic: 'Friends',
      body: 'Bring your book to the park and read next to other people who also want to make friends without making small talk.',
      score: 28,
      replies: [
        {
          id: 'blanket',
          author: 'Sunday person',
          body: 'Can we bring a blanket and an unreasonable amount of snacks?',
        },
      ],
    },
    {
      id: 'soup',
      author: 'Night picnic',
      headline: 'Soup night this Thursday?',
      topic: 'Plans',
      body: 'Thinking a potluck where everyone brings a soup and a deeply unserious opinion.',
      score: 22,
      replies: [],
    },
    {
      id: 'hat',
      author: 'Anonymous neighbor',
      headline: 'To the tiny hat fairy',
      topic: 'Chatter',
      body: 'To whoever keeps putting tiny crocheted hats on the fence posts: this neighborhood is better because you’re in it.',
      score: 64,
      replies: [],
    },
  ];
  let posts = $state<Post[]>(samples),
    saved = $state<string[]>([]),
    hidden = $state<string[]>([]),
    tab = $state('nearby');
  let draft = $state(''),
    headline = $state(''),
    replyTo = $state<Post | null>(null);
  let profile = $derived<Profile>({
    alias: 'You',
    join: { community: 'preview' },
    posts,
    saved,
    hideAnonymous: false,
    blocks: [],
    pendingUnblocks: [],
    outbox: [],
  });
  const reading=$derived((page.state as {sample?:{id:string; comments:boolean}}).sample);
  function openThread(post:Post,comments:boolean){
    replaceState('',{sampleScroll:window.scrollY});
    pushState('',{sample:{id:post.id,comments}});
  }
  $effect(()=>{
    const state=page.state as {sample?:{id:string;comments:boolean};sampleScroll?:number};
    void tick().then(()=>{
      if(page.state!==state)return;
      if(state.sample) {
        const target=document.getElementById(state.sample.comments?'thread-comments':'demo-thread');
        target?.focus({preventScroll:true});target?.scrollIntoView();
      } else if(state.sampleScroll!==undefined)window.scrollTo(0,state.sampleScroll);
    });
  });
  let replyDialog: HTMLDialogElement;
  function reply(post: Post) {
    replyTo = post;
    draft = '';
    replyDialog.showModal();
  }
  function sendReply(e: SubmitEvent) {
    e.preventDefault();
    if (!draft.trim()) return;
    posts = posts.map((p) =>
      p.id === replyTo?.id
        ? {
            ...p,
            replies: [
              ...p.replies,
              { id: crypto.randomUUID(), mine: true, author: 'You', body: draft.trim() },
            ],
          }
        : p,
    );
    replyDialog.close();
    draft = '';
  }
  async function vote(post: Post, direction: number) {
    const value = post.vote === direction ? 0 : direction;
    posts = posts.map((p) =>
      p.id === post.id ? { ...p, vote: value, score: (p.score || 0) - (p.vote || 0) + value } : p,
    );
  }
  function hide(id: string) {
    hidden = [...hidden, id];
  }
</script>

<AppHead />
<svelte:head><title>Local demo · Yap</title></svelte:head>
<header class="app-header">
  <div class="bar">
    <a class="quiet" href="/app/">Back to Yap</a>
    <a href="/" class="brand" aria-label="Yap home"><img src="/assets/yap-wordmark.svg" width="72" height="36" alt="yap" /></a>
  </div>
</header>
<main id="neighborhood" inert={!ready} aria-busy={!ready}>
  <div class="heading" class:thread-heading={!!reading}>
    {#if reading}<ActionButton onclick={()=>history.back()}>Back to demo</ActionButton><h1 id="demo-thread" tabindex="-1">Conversation</h1>
    {:else}<h1>Local demo</h1>
    <Sheet
      title="Write a sample post"
      label="Post"
      prepare={() => {
        draft = '';
        headline = '';
      }}
      >{#snippet children(close)}
        <form
          onsubmit={(e) => {
            e.preventDefault();
            if (!headline.trim() || !draft.trim()) return;
            posts = [
              {
                id: crypto.randomUUID(),
                author: 'You',
                mine: true,
                headline: headline.trim(),
                topic: 'Chatter',
                body: draft.trim(),
                replies: [],
              },
              ...posts,
            ];
            tab = 'nearby';
            close();
          }}
        >
          <label>Headline<input bind:value={headline} maxlength="80" required /></label><label
            >Post<textarea bind:value={draft} maxlength="500" required></textarea></label
          >
          <ActionButton variant="filled">Add to demo</ActionButton>
        </form>{/snippet}</Sheet
    >
  {/if}</div>
  <p class="hint">Sample conversations. Your changes stay in this preview until you leave.</p>
  <Tabs.Root bind:value={tab}
    ><Tabs.List hidden={!!reading} class="dm-tabs mtrl-tabs mtrl-tabs--primary" aria-label="Demo feeds"
      ><Tabs.Trigger class="mtrl-button mtrl-tab mtrl-tab--text-only" value="nearby">Nearby</Tabs.Trigger><Tabs.Trigger class="mtrl-button mtrl-tab mtrl-tab--text-only" value="saved"
        >Saved</Tabs.Trigger
      ></Tabs.List
    >
    {#each ['nearby', 'saved'] as value}<Tabs.Content {value}
        >{#if value === tab}
          {#each posts.filter((p) => (!reading || p.id===reading.id) && !hidden.includes(p.id) && (value === 'nearby' || saved.includes(p.id))) as post (post.id)}
            <FeedPost
              {post}
              {profile}
              preview {openThread} detail={!!reading}
              {vote}
              {reply}
              {hide}
              mute={(p) => hide(p.id)}
              save={(p) =>
                (saved = saved.includes(p.id)
                  ? saved.filter((id) => id !== p.id)
                  : [...saved, p.id])}
              remove={(id) => {
                hide(id);
                posts = posts.map((p) => ({ ...p, replies: p.replies.filter((r) => r.id !== id) }));
              }}
              replica={false}
            />
          {:else}<p>
              {value === 'saved' ? 'Save a sample post to find it here.' : 'No posts here yet.'}
            </p>{/each}
        {/if}</Tabs.Content
      >{/each}
  </Tabs.Root>
</main>
<dialog bind:this={replyDialog} aria-labelledby="demo-reply-title">
  <form onsubmit={sendReply}>
    <div class="dialog-title">
      <h2 id="demo-reply-title">Write a reply</h2>
      <ActionButton type="button" onclick={() => replyDialog.close()}>Close</ActionButton>
    </div>
    <label>Reply<textarea bind:value={draft} required maxlength="500"></textarea></label><ActionButton
      variant="filled">Add reply</ActionButton
    >
  </form>
</dialog>
