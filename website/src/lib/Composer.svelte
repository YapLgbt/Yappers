<script module lang="ts">
  import type { Area, PublicProfile } from './community';

  export type WrittenAction = { body: string; displayName: string } & (
    | { kind: 'reply'; parent: string }
    | { kind: 'postV2'; headline: string; topic: string; anonymous: boolean; profile?: PublicProfile; area?: Area }
  );
</script>

<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import { flushSync, tick } from 'svelte';
  import { authorLabel, errorMessage, topics, type Profile, type Post } from './community';

  let { profile, domain, chooseUsername, queue, deliver, showReply, destination }: {
    profile: Profile;
    queue: (action: WrittenAction, scope: string) => string | Promise<string>;
    domain?: string | null;
    chooseUsername?: () => void;
    deliver?: () => Promise<void>;
    showReply?: (parent: string, id: string) => void;
    destination?: string;
  } = $props();
  let dialog: HTMLDialogElement;
  let opener: HTMLButtonElement;
  let discardDialog: HTMLDialogElement;
  let headlineField: HTMLInputElement;
  let bodyField: HTMLTextAreaElement;
  let parent = $state<Post | null>(null);
  let headline = $state(''), body = $state(''), topic = $state(topics[0]);
  let anonymous = $state(false), error = $state(''), sending = $state(false);
  let submitted = $state(false);
  let editing = $state(false), reserveScrollbar = $state(false);

  // On mobile the software keyboard can resize only the visual viewport.
  $effect(() => {
    if (!editing || !window.visualViewport) return;
    const viewport = window.visualViewport;
    function fitKeyboard() {
      if (viewport.scale === 1) {
        dialog.style.setProperty('--editor-top', viewport.offsetTop + 'px');
        dialog.style.setProperty('--editor-height', viewport.height + 'px');
      }
    }
    fitKeyboard();
    viewport.addEventListener('resize', fitKeyboard);
    viewport.addEventListener('scroll', fitKeyboard);
    return () => {
      viewport.removeEventListener('resize', fitKeyboard);
      viewport.removeEventListener('scroll', fitKeyboard);
      dialog.style.removeProperty('--editor-top');
      dialog.style.removeProperty('--editor-height');
    };
  });
  let scope = $state('');
  let displayName = $derived(profile.alias?.trim() || '');
  let needsUsername = $derived(!!chooseUsername && !anonymous && !profile.username);
  let headlineError = $derived(submitted && !parent && !headline.trim() ? 'Enter a headline.' : '');
  let bodyError = $derived(submitted && !body.trim() ? (parent ? 'Write a reply.' : 'Write your post.') : '');
  let nameProblem = $derived.by(() => {
    if (anonymous) return '';
    const length = Array.from(displayName).length;
    if (length < 2 || length > 30) return 'Set a display name in You → Edit profile to post by name.';
    if (parent && displayName === 'Anonymous neighbor')
      return 'Choose another display name in You → Edit profile to reply by name. This one identifies anonymous replies.';
    return '';
  });

  export function open(post: Post | null, trigger: HTMLButtonElement) {
    if (dialog.open) return;
    opener = trigger;
    scope = post?.scope || profile.activeGroup || profile.join.community;
    parent = post;
    body = '';
    headline = '';
    topic = topics[0];
    anonymous = false;
    error = '';
    submitted = false;
    reserveScrollbar = window.innerWidth > document.documentElement.clientWidth;
    editing = true;
    flushSync();
    dialog.showModal();
    (post ? bodyField : headlineField).focus({ preventScroll: true });
  }
  function discardDraft(restoreFocus = true) {
    body = '';
    headline = '';
    dialog.close();
    if (restoreFocus && opener.isConnected) opener.focus({ preventScroll: true });
  }
  function dismiss() {
    if (sending) return;
    if (body.trim() || headline.trim()) {
      if (!discardDialog.open) discardDialog.showModal();
      return;
    }
    discardDraft();
  }
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (sending || needsUsername || nameProblem) return;
    submitted = true;
    error = '';
    const invalid = headlineError ? headlineField : bodyError ? bodyField : null;
    if (invalid) {
      await tick();
      invalid.focus();
      return;
    }
    sending = true;
    try {
      const fields = { body: body.trim(), displayName: anonymous ? 'Anonymous neighbor' : displayName };
      const parentId = parent?.id;
      const action: WrittenAction = parentId
        ? { kind: 'reply', parent: parentId, ...fields }
        : {
            kind: 'postV2', headline: headline.trim(), topic, anonymous, ...fields,
            ...(!anonymous && profile.publicProfile && { profile: profile.publicProfile }),
            ...(!profile.activeGroup && profile.nearby && { area: profile.nearby.center }),
          };
      const eventId = await queue(action, scope);
      discardDraft(!parentId);
      parent = null;
      if (parentId) showReply?.(parentId, eventId);
    } catch (problem) { error = errorMessage(problem); return; }
    finally { sending = false; }
    await deliver?.();
  }
</script>
<dialog id="editor" class:reserveScrollbar bind:this={dialog} aria-labelledby="editor-title" onclose={() => editing = dialog.open}
  oncancel={event => { event.preventDefault(); dismiss(); }}>
  <form id="editor-form" novalidate onsubmit={submit}>
    <div class="composer-bar">
      <ActionButton type="button" data-close onclick={dismiss} disabled={sending}>Close</ActionButton>
      <h2 id="editor-title">{parent ? 'New reply' : 'New post'}</h2>
      <ActionButton id="send" variant="filled" type="submit" disabled={sending || needsUsername || !!nameProblem}>{sending ? 'Sending…' : parent ? 'Reply' : 'Post'}</ActionButton>
    </div>
    <div class="composer-signature">
      <div class="signature-preview" id="public-byline">
        <span>{parent ? 'Replying as' : 'Posting as'}</span>
        <strong>{anonymous ? 'Anonymous neighbor' : displayName || 'Display name required'}</strong>
        {#if anonymous}
          <span class="signature-handle">{parent ? 'This reply only.' : 'This post only.'}{chooseUsername ? ' Your home can identify you.' : ''}</span>
        {:else if profile.username}<span class="signature-handle">{profile.username}</span>{/if}
      </div>
      <label class="anon-choice check">
        <span>Anonymous mode</span>
        <input type="checkbox" role="switch" bind:checked={anonymous} disabled={sending}
          aria-describedby="public-byline" />
      </label>
    </div>
    {#if needsUsername && domain}
      <div id="editor-username"><ActionButton type="button" onclick={chooseUsername}>Choose a username to post by name</ActionButton></div>
    {:else if needsUsername}
      <p class="composer-help">Posting by name requires a username on this home. You can also post anonymously.</p>
    {/if}
    {#if nameProblem}<p class="composer-help">{nameProblem}</p>{/if}
    {#if error}<p id="editor-error" class="form-feedback form-error" role="alert">{error}</p>{/if}
    <div class="composer-content">
      <div class="writing-area">
        <div class="composer-context">
          {#if parent}
            <p>Replying to <strong>{authorLabel(parent)}</strong></p>
            <blockquote>{parent.headline || parent.body}</blockquote>
          {:else}
            <p>Posting to <strong>{destination || profile.joinedGroups?.[scope]?.group.name || profile.nearby?.center.label || 'All cities'}</strong></p>
          {/if}
        </div>
        <div class="writing-field" id="headline-row" hidden={!!parent}>
          <label for="headline" class="sr-only">Headline</label>
          <input id="headline" class="input" bind:this={headlineField} bind:value={headline}
            placeholder="Give your post a headline" required={!parent} readonly={sending}
            maxlength="80" aria-invalid={headlineError ? true : undefined}
            aria-describedby={headlineError ? 'headline-error headline-limit' : 'headline-limit'}>
          <span id="headline-limit" class="sr-only">80 characters maximum</span>
          {#if headlineError || headline.length >= 60}
            <div class="field-support">
              {#if headlineError}<span id="headline-error" class="field-error">{headlineError}</span>{/if}
              {#if headline.length >= 60}<span class="field-count">{headline.length} / 80</span>{/if}
            </div>
          {/if}
        </div>
        <div class="writing-field body-field">
          <label id="body-label" for="body" class="sr-only">{parent ? 'Reply' : 'Post'}</label>
          <textarea id="body" class="textarea" bind:this={bodyField} bind:value={body}
            placeholder={parent ? 'Write your reply…' : 'What’s on your mind?'} maxlength="500" rows="4"
            required readonly={sending} aria-invalid={bodyError ? true : undefined}
            aria-describedby={bodyError ? 'body-error body-limit' : 'body-limit'}></textarea>
          <span id="body-limit" class="sr-only">500 characters maximum</span>
          <div class="field-support">
            {#if bodyError}<span id="body-error" class="field-error">{bodyError}</span>{/if}
            <span class="field-count" aria-hidden="true">{body.length} / 500</span>
          </div>
        </div>
      </div>
      {#if !parent}
        <div class="composer-options">
          <div class="option-row" id="topic-row">
            <label for="topic">Topic</label>
            <select id="topic" class="select" bind:value={topic} disabled={sending}>
              {#each topics as value (value)}<option>{value}</option>{/each}
            </select>
          </div>
        </div>
      {/if}
    </div>
  </form>
</dialog>
<dialog id="discard-draft" bind:this={discardDialog} aria-labelledby="discard-draft-title" aria-describedby="discard-draft-detail">
  <div class="task-form">
    <h2 id="discard-draft-title">Discard your draft?</h2>
    <p id="discard-draft-detail">What you’ve written will be lost.</p>
    <div class="form-actions form-actions--footer">
      <ActionButton type="button" onclick={() => discardDialog.close()}>Keep editing</ActionButton>
      <ActionButton type="button" variant="danger" onclick={() => { discardDialog.close(); discardDraft(); }}>Discard draft</ActionButton>
    </div>
  </div>
</dialog>

<style>
  #editor {
    --inset: 2rem;
    width: min(39rem, calc(100% - 2rem));
    max-height: min(48rem, calc(100dvh - 3rem));
    padding: 0;
    background: var(--bg);
    border-radius: 1rem;
    overflow: clip;
  }
  #editor[open], #editor-form { display: flex; flex-direction: column; min-block-size: 0 }
  .composer-bar {
    flex: none;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    gap: .375rem;
    align-items: center;
    padding: .625rem 1rem;
    border-block-end: 1px solid var(--line);
  }
  .composer-bar :global([data-close]) { justify-self: start }
  .composer-bar :global(#send) { justify-self: end }
  #editor-title { margin: 0; text-align: center; font: var(--type-title) }
  #editor-error { flex: none; margin: .5rem var(--inset) 0 }
  .composer-content {
    display: flex;
    flex-direction: column;
    min-block-size: 0;
    padding: 1.25rem var(--inset) 1.5rem;
    overflow-y: auto;
    overscroll-behavior: contain;
    scroll-padding-block: 1rem;
  }
  .writing-area { display: flex; flex-direction: column; flex: 1 0 auto }
  .composer-context { margin-block-end: 1rem; color: var(--quiet); font: var(--type-small); overflow-wrap: anywhere }
  .composer-context p { margin: 0 }
  .composer-context strong { color: var(--ink); font-weight: 600 }
  .composer-context blockquote {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    margin: .375rem 0 0;
    padding-inline-start: .75rem;
    border-inline-start: 1px solid var(--line);
    font: var(--type-post-body);
  }
  .writing-field { position: relative; min-inline-size: 0 }
  .body-field { display: flex; flex: 1; flex-direction: column }
  #headline, #body {
    min-width: 0;
    width: 100%;
    padding: .375rem 0;
    margin: 0;
    border: 0;
    border-radius: 0;
    box-shadow: none;
    color: var(--ink);
    background: transparent;
    letter-spacing: normal;
  }
  #headline { font: var(--type-post-title); min-height: 3rem }
  #body { flex: 1; min-height: 8.5rem; font: var(--type-post-body); resize: vertical }
  #editor :is(#headline, #body)::placeholder { color: var(--quiet); opacity: .85 }
  #editor :is(#headline, #body):focus-visible { outline: 0; box-shadow: inset 0 -1px var(--plum) }
  #editor :is(#headline, #body)[aria-invalid=true] { box-shadow: inset 0 -1px var(--error) }
  .field-support { display: flex; gap: .5rem; color: var(--quiet); font: var(--type-caption) }
  .field-count { margin-inline-start: auto; font-variant-numeric: tabular-nums }
  .field-error { color: var(--error); font: var(--type-small) }
  .composer-options { border-block-start: 1px solid var(--line); margin-block-start: 1rem; padding-block-start: .5rem }
  .composer-signature {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: .75rem;
    padding: .375rem var(--inset);
    border-block-end: 1px solid var(--line);
    background: var(--wash);
  }
  .signature-preview { display: grid; min-width: 0; font: var(--type-small); color: var(--quiet) }
  .signature-handle { overflow-wrap: anywhere }
  .signature-preview strong { color: var(--ink); font: var(--type-label); overflow-wrap: anywhere }
  #editor .anon-choice {
    display: flex;
    align-items: center;
    flex: none;
    gap: .5rem;
    min-height: 48px;
    margin: 0;
    font: var(--type-label);
    cursor: pointer;
  }
  .anon-choice input {
    appearance: none;
    display: grid;
    align-items: center;
    flex: none;
    width: 2.75rem;
    height: 1.625rem;
    margin: 0;
    padding: 2px;
    border: 1px solid var(--quiet);
    border-radius: 1rem;
    color: var(--quiet);
    background: var(--wash);
    cursor: inherit;
  }
  .anon-choice input::before {
    content: '';
    width: 1.125rem;
    height: 1.125rem;
    border-radius: 50%;
    background: currentColor;
  }
  .anon-choice input:checked { color: var(--on-plum); background: var(--plum); border-color: var(--plum) }
  .anon-choice input:checked::before { justify-self: end }
  .anon-choice input:focus-visible { outline: 2px solid var(--plum); outline-offset: 2px }
  .anon-choice:has(:disabled) { opacity: .6; cursor: default }
  @media (max-width: 25rem) {
    .composer-signature { display: grid; grid-template-columns: minmax(0, 1fr) auto; column-gap: .5rem; row-gap: 0 }
    .signature-preview { display: contents }
    .signature-preview > span:first-child { grid-area: 1 / 1; font: var(--type-caption); color: var(--quiet) }
    .signature-preview strong { grid-area: 2 / 1 }
    .signature-handle { grid-area: 3 / 1 / 4 / -1; font: var(--type-caption); color: var(--quiet) }
    #editor .anon-choice { grid-area: 1 / 2 / 3 / 3 }
  }
  @media (forced-colors: active) {
    .anon-choice input { appearance: auto }
    .anon-choice input::before { content: none }
  }
  .option-row {
    display: grid;
    grid-template-columns: minmax(5.5rem, auto) minmax(0, 1fr);
    gap: .375rem 1rem;
    align-items: center;
    padding-block: .25rem;
  }
  #editor .option-row label { margin: 0; font: var(--type-label) }
  .option-row select {
    min-height: 3rem;
    width: 100%;
    min-width: 0;
    margin: 0;
    border: 1px solid var(--line);
    border-radius: .5rem;
    background-color: var(--bg);
    font: var(--type-body);
    color: var(--ink);
    text-overflow: ellipsis;
  }
  .option-row select { appearance: auto; background-image: none; padding: .5rem }
  .option-row select:focus-visible { outline: 2px solid var(--plum); outline-offset: -2px }
  #editor-username { margin-inline: var(--inset) }
  .composer-help { margin: .625rem var(--inset) 0; color: var(--quiet); font: var(--type-small) }
  #discard-draft { max-inline-size: 26rem; background: var(--bg) }

  @media (max-width: 599px), (pointer: coarse) and (max-height: 540px) {
    dialog#editor[open] {
      --inset: max(1.25rem, env(safe-area-inset-left), env(safe-area-inset-right));
      inset: var(--editor-top, 0px) 0 auto;
      width: 100%;
      max-width: none;
      height: var(--editor-height, 100dvh);
      max-height: none;
      margin: 0;
      padding: 0;
      border-radius: 0;
    }
    #editor-form { height: 100% }
    .composer-bar {
      padding: max(.5rem, env(safe-area-inset-top)) max(.75rem, env(safe-area-inset-right)) .5rem max(.75rem, env(safe-area-inset-left));
    }
    .composer-content { flex: 1; padding-block-end: max(1.25rem, env(safe-area-inset-bottom)) }
    #body { resize: none }
  }
  @media (forced-colors: active) {
    #editor :is(#headline, #body):focus-visible { outline: 2px solid Highlight; outline-offset: -2px }
  }
</style>
