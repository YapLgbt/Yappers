<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import { tick } from 'svelte';
  import { errorMessage, topics, type Community, type Profile, type Post } from './community';
  let { client, profile, chooseUsername, deliver, showReply }: {
    client: Community; profile: Profile; showReply:(parent:string,id:string)=>void; chooseUsername: () => void; deliver: () => Promise<void>;
  } = $props();
  let dialog: HTMLDialogElement;
  let discardDialog: HTMLDialogElement;
  let headlineField: HTMLInputElement;
  let bodyField: HTMLTextAreaElement;
  let aliasField: HTMLInputElement;
  let parent = $state<Post | null>(null);
  let headline = $state(''), body = $state(''), topic = $state(topics[0]), alias = $state('');
  let identity = $state('anonymous'), error = $state(''), sending = $state(false);
  let submitted = $state(false);
  let scope = $state('');
  let named = $derived(identity === 'named');
  let domain = $derived(client.usernameDomain(profile));
  let headlineError = $derived(submitted && !parent && !headline.trim() ? 'Enter a headline.' : '');
  let bodyError = $derived(submitted && !body.trim() ? (parent ? 'Write a reply.' : 'Write your post.') : '');
  let aliasError = $derived(submitted && named && alias.trim().length < 2 ? 'Use at least two characters.' : '');

  export async function open(post: Post | null) {
    if (scope !== (profile.activeGroup || profile.join.community) || parent?.id !== post?.id) { body = ''; headline = ''; }
    scope = post?.scope || profile.activeGroup || profile.join.community; parent = post;
    identity = profile.postAnonymously === false ? 'named' : 'anonymous';
    alias = profile.alias;
    error = '';
    submitted = false;
    dialog.showModal();
    await tick();
    (post ? bodyField : headlineField).focus();
  }
  function discardDraft() {
    body='';headline='';dialog.close();
  }
  function dismiss() {
    if (sending) return;
    if (body.trim() || headline.trim()) {
      if (!discardDialog.open) discardDialog.showModal();
      return;
    }
    discardDraft();
  }
  async function choose(event: Event) {
    identity = (event.currentTarget as HTMLSelectElement).value;
    const anonymous = identity === 'anonymous';
    try { await client.modify(s => ({ ...s, postAnonymously: anonymous })); }
    catch { error = 'Couldn’t remember your choice for next time.'; }
  }
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (sending) return;
    submitted = true;
    error = '';
    const invalid = headlineError ? headlineField : bodyError ? bodyField : aliasError ? aliasField : null;
    if (invalid) {
      await tick();
      invalid.focus();
      return;
    }
    sending = true;
    try {
      const fields = { body: body.trim(), displayName: named ? alias.trim() : 'Anonymous neighbor' };
      const parentId=parent?.id;
      const eventId=await client.queue(parent ? { kind: 'reply', parent: parent.id, ...fields }
        : { kind: 'postV2', headline: headline.trim(), topic, anonymous: !named, ...fields, ...(named && profile.publicProfile && {profile:profile.publicProfile}), ...(!profile.activeGroup && profile.nearby && {area:profile.nearby.center}) }, named ? fields.displayName : null,scope);
      body = ''; headline = ''; parent = null; dialog.close();
      if(parentId)showReply(parentId,eventId);
      await deliver();
    } catch (problem) { error = errorMessage(problem); }
    finally { sending = false; }
  }
</script>
<dialog oncancel={e=>{e.preventDefault();dismiss()}} id="editor" bind:this={dialog} aria-labelledby="editor-title">
  <form id="editor-form" novalidate onsubmit={submit}>
    <div class="dialog-title">
      <div>
        <h2 id="editor-title">{parent ? 'New reply' : 'New post'}</h2>
        {#if !parent}<p class="composer-context">Posting to {profile.joinedGroups?.[scope]?.group.name || profile.nearby?.center.label || 'All cities'}</p>{/if}
      </div>
      <ActionButton type="button" data-close onclick={dismiss} disabled={sending}>Close</ActionButton>
    </div>
    <div class="composer-fields">
      <div class="composer-field" id="headline-row" hidden={!!parent}>
        <label for="headline">Headline</label>
        <input id="headline" class="editorial-input headline-input" bind:this={headlineField} bind:value={headline} required={!parent} readonly={sending} maxlength="80" aria-invalid={headlineError ? true : undefined} aria-describedby={headlineError ? 'headline-error headline-limit' : 'headline-limit'}>
        <span id="headline-limit" class="sr-only">80 characters maximum</span>
        <div class="field-support">
          {#if headlineError}<span id="headline-error" class="field-error">{headlineError}</span>{/if}
          <span class="field-count" aria-hidden="true">{headline.length} / 80</span>
        </div>
      </div>
      <div class="composer-field">
        <label id="body-label" for="body">{parent ? 'Reply' : 'Post'}</label>
        <textarea id="body" class="editorial-input body-input" bind:this={bodyField} bind:value={body} maxlength="500" rows="4" required readonly={sending} aria-invalid={bodyError ? true : undefined} aria-describedby={bodyError ? 'body-error body-limit' : 'body-limit'}></textarea>
        <span id="body-limit" class="sr-only">500 characters maximum</span>
        <div class="field-support">
          {#if bodyError}<span id="body-error" class="field-error">{bodyError}</span>{/if}
          <span class="field-count" aria-hidden="true">{body.length} / 500</span>
        </div>
      </div>
      <div class="composer-options">
        <div class="composer-field" id="topic-row" hidden={!!parent}>
          <label for="topic">Topic</label>
          <select id="topic" bind:value={topic} disabled={sending}>{#each topics as value (value)}<option>{value}</option>{/each}</select>
        </div>
        <div class="composer-field">
          <label id="identity-label" for="posting-identity">{parent ? 'Reply as' : 'Post as'}</label>
          <select id="posting-identity" value={identity} disabled={sending} onchange={choose} aria-describedby={named ? 'identity-default' : 'identity-hint identity-default'}>
            <option value="anonymous">Anonymous</option><option value="named" disabled={!domain && !profile.username}>{profile.username || (domain ? 'My username' : profile.alias)}</option>
          </select>
        </div>
      </div>
      <div class="composer-field" id="name-row" hidden={!named}>
        <label for="alias">Display name</label>
        <input id="alias" bind:this={aliasField} bind:value={alias} required={named} readonly={sending} maxlength="30" autocomplete="off" aria-invalid={aliasError ? true : undefined} aria-describedby={aliasError ? 'alias-error' : undefined}>
        {#if aliasError}<div class="field-support"><span id="alias-error" class="field-error">{aliasError}</span></div>{/if}
      </div>
      <div id="editor-username" hidden={!named || !domain || !!profile.username}><ActionButton type="button" onclick={chooseUsername}>Choose a username</ActionButton></div>
      <p class="composer-help">
        <span id="identity-hint" hidden={named}>Your home can identify anonymous posts.</span>
        <span id="identity-default">This posting choice applies to this browser.</span>
      </p>
    </div>
    <div class="composer-actions">
      <p id="editor-error" class="form-feedback form-error" role="alert">{error}</p>
      <ActionButton id="send" variant="filled" type="submit" disabled={sending || (named && !profile.username)}>{sending ? 'Sending…' : parent ? 'Reply' : 'Post'}</ActionButton>
    </div>
  </form>
</dialog>
<dialog id="discard-draft" bind:this={discardDialog} aria-labelledby="discard-draft-title" aria-describedby="discard-draft-detail">
  <div class="task-form">
    <h2 id="discard-draft-title">Discard your draft?</h2>
    <p id="discard-draft-detail">What you’ve written will be lost.</p>
    <div class="form-actions form-actions--footer">
      <ActionButton type="button" onclick={() => discardDialog.close()}>Keep editing</ActionButton>
      <ActionButton type="button" variant="filled" onclick={() => { discardDialog.close(); discardDraft(); }}>Discard draft</ActionButton>
    </div>
  </div>
</dialog>

<style>
  .dialog-title { flex: none; align-items: start; margin-block-end: .875rem; }
  #editor-title { margin-block: 0; }
  .composer-context { margin: .25rem 0 0; color: var(--mtrl-sys-color-on-surface-variant); font: var(--type-small); }
  .composer-fields { display: grid; gap: 1.25rem; padding: .625rem .375rem; margin-inline: -.375rem; }
  .composer-options { display: grid; gap: 1.25rem; }
  .composer-field { position: relative; min-inline-size: 0; }

  #editor .composer-field label {
    position: absolute;
    z-index: 1;
    inset-block-start: 0;
    inset-inline-start: .75rem;
    transform: translateY(-50%);
    margin: 0;
    padding-inline: .25rem;
    background: var(--mtrl-sys-color-surface-container-high);
    color: var(--mtrl-sys-color-on-surface-variant);
    font: var(--type-caption);
  }

  #editor .composer-field:focus-within label { color: var(--mtrl-sys-color-primary); }
  #editor .composer-field:has([aria-invalid="true"]) label { color: var(--mtrl-sys-color-error); }
  .composer-field :is(input, textarea, select) { display: block; margin-block-start: 0; }
  .composer-field .editorial-input { letter-spacing: normal; }
  .composer-field .headline-input { font: var(--type-post-title); }
  .composer-field .body-input { min-block-size: 9rem; font: var(--type-post-body); }

  .field-support {
    display: flex;
    gap: .5rem;
    padding: .25rem .25rem 0;
    color: var(--mtrl-sys-color-on-surface-variant);
    font: var(--type-caption);
  }

  .field-count { margin-inline-start: auto; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .field-error { color: var(--mtrl-sys-color-error); }
  .composer-help { margin: -.75rem 0 0; color: var(--mtrl-sys-color-on-surface-variant); font: var(--type-small); }
  .composer-help span { display: block; }
  #editor-username { margin-block-start: -.75rem; }

  .composer-actions {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: .5rem 1rem;
    margin-block-start: .625rem;
    padding-block: .75rem .25rem;
    border-block-start: 1px solid var(--mtrl-sys-color-outline-variant);
    background: var(--mtrl-sys-color-surface-container-high);
  }

  #editor-error { flex: 1 1 14rem; margin: 0; font: var(--type-small); }
  #editor-error:empty { flex: 0; padding: 0; }
  #discard-draft { max-inline-size: 26rem; }
  #discard-draft .form-actions {
    --mtrl-sys-color-primary: var(--mtrl-sys-color-error);
    --mtrl-sys-color-on-primary: var(--mtrl-sys-color-on-error);
  }

  @media (min-width: 520px) {
    .composer-options:has(#topic-row:not([hidden])) { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }

  @media (min-height: 480px) {
    #editor[open] { display: flex; flex-direction: column; overflow: clip; }
    #editor-form { display: flex; flex-direction: column; min-block-size: 0; }
    .composer-fields { overflow-y: auto; overscroll-behavior: contain; scroll-padding-block: .625rem; }
  }
</style>
