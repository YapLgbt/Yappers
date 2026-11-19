<script lang="ts">
  import ActionButton from '../../../lib/ActionButton.svelte';
  import { onMount,tick } from 'svelte';
  import {page} from '$app/state';
  import {pushState,replaceState} from '$app/navigation';
  import { Tabs } from '@skeletonlabs/skeleton-svelte';
  import { trackFeed } from '../../../lib/feedMotion.svelte';
  import { pageMotion } from '../../../lib/pageMotion';
  import AppHead from '../../../lib/AppHead.svelte';
  import FeedPost from '../../../lib/FeedPost.svelte';
  import Composer, { type WrittenAction } from '../../../lib/Composer.svelte';
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
  let composer = $state<Composer>();
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
  let feedList = $state<HTMLDivElement>();
  const shown = $derived(posts.filter(post => (!reading || post.id === reading.id) &&
    !hidden.includes(post.id) && (tab === 'nearby' || saved.includes(post.id))));
  trackFeed(() => feedList, () => ready && !reading ? tab : null, () => shown);
  function openThread(post:Post,comments:boolean){
    replaceState('', {sampleScroll: window.scrollY, sampleTrigger: (comments ? 'comments-' : 'read-') + post.id});
    pushState('',{sample:{id:post.id,comments}});
  }
  $effect(()=>{
    const state=page.state as {sample?:{id:string;comments:boolean};sampleScroll?:number; sampleTrigger?:string};
    void tick().then(()=>{
      if(page.state!==state)return;
      if(state.sample) {
        const target=document.getElementById(state.sample.comments?'thread-comments':'demo-thread');
        target?.focus({preventScroll:true});target?.scrollIntoView();
      } else if (state.sampleScroll !== undefined) {
        if (state.sampleTrigger) document.getElementById(state.sampleTrigger)?.focus({preventScroll: true});
        window.scrollTo(0, state.sampleScroll);
      }
    });
  });
  function addSample(action: WrittenAction) {
    const id = crypto.randomUUID();
    const byYou = { id, mine: true, author: action.displayName, body: action.body };
    if (action.kind === 'reply') {
      posts = posts.map(post => post.id === action.parent
        ? { ...post, replies: [...post.replies, byYou] }
        : post);
    } else {
      posts = [{ ...byYou, headline: action.headline, topic: action.topic, replies: [] }, ...posts];
      tab = 'nearby';
    }
    return id;
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

<AppHead title="Local demo · Yap" />
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
    <ActionButton variant="filled" onclick={event => composer?.open(null, event.currentTarget)}>Post</ActionButton>
  {/if}</div>
  <p class="hint">Sample conversations. Your changes stay in this preview until you leave.</p>
  <Tabs value={tab} onValueChange={({ value }) => tab = value}
    ><Tabs.List hidden={!!reading} class="dm-tabs" aria-label="Demo feeds"
      ><Tabs.Trigger class="btn" value="nearby">Nearby</Tabs.Trigger><Tabs.Trigger class="btn" value="saved"
        >Saved</Tabs.Trigger
      ></Tabs.List
    >
    <div use:pageMotion={(tab === 'nearby' ? 0 : 2) + Number(!!reading)}>
    {#each ['nearby', 'saved'] as value}<Tabs.Content {value}
        >{#if value === tab}
          <div class="demo-posts" bind:this={feedList}>
          {#each shown as post (post.id)}
            <FeedPost
              {post}
              {profile}
              preview {openThread} detail={!!reading}
              {vote}
              reply={(post, trigger) => composer?.open(post, trigger)}
              {hide}
              mute={(p) => hide(p.id)}
              save={(p) => {
                saved = saved.includes(p.id)
                  ? saved.filter((id) => id !== p.id)
                  : [...saved, p.id];
              }}
              remove={(id) => {
                hide(id);
                posts = posts.map((p) => ({ ...p, replies: p.replies.filter((r) => r.id !== id) }));
              }}
              replica={false}
            />
          {:else}<p>
              {value === 'saved' ? 'Save a sample post to find it here.' : 'No posts here yet.'}
            </p>{/each}
          </div>
        {/if}</Tabs.Content
      >{/each}
    </div>
  </Tabs>
</main>
<Composer bind:this={composer} {profile} destination="Local demo" queue={addSample}
  showReply={async (_parent, id) => {
    await tick();
    const reply = document.getElementById('reply-' + id);
    reply?.focus({ preventScroll: true });
    reply?.scrollIntoView({ block: 'nearest' });
  }} />
