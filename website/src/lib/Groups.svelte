<script lang="ts">
  import { pageMotion } from './pageMotion';
  import ActionButton from './ActionButton.svelte';
  import { onMount } from 'svelte';
  import { Tabs } from 'bits-ui';
  import AppIcon from './AppIcon.svelte';
  import Sheet from './Sheet.svelte';
  import PrivateGroups from './PrivateGroups.svelte';
  import {
    topics,
    errorMessage,
    type Profile,
    type Community,
    type GroupDetails, type Locality,
  } from './community';
  let {
    client,
    profile,
    openGroup,
  }: { client: Community; profile: Profile; openGroup: (id: string | null) => Promise<void> } =
    $props();
  const tabs = ['joined', 'local', 'world'];
  let showingPrivate = $state(false);
  const formId = $props.id();
  let searchInput = $state<HTMLInputElement>();
  let tab = $state('joined'),
    query = $state(''),
    browseCategory = $state(''),
    name = $state(''),
    description = $state(''),
    category = $state(topics[0]);
  let local = $state(false),
    city = $state(''),
    region = $state(''),
    country = $state(''),
    busy = $state(false),
    error = $state(''),
    formError = $state('');
  let townQuery=$state(''), townError=$state('');
  function townLabel(place:Locality){return [place.city,place.region,place.country].filter(Boolean).join(', ');}
  const towns=$derived([...new Map((profile.groupDirectory || []).flatMap(group=>group.locality ?
    [[townLabel(group.locality).toLowerCase(),group.locality] as const] : [])).values()]
    .filter(place=>townLabel(place).toLowerCase().includes(townQuery.toLowerCase().trim()))
    .sort((a,b)=>townLabel(a).localeCompare(townLabel(b))));
  async function selectTown(town:Locality|null, close:()=>void) {
    try { await client.modify(s=>({...s,groupLocality:town})); close(); }
    catch { townError='Couldn’t save this town. Try again.'; }
  }
  let joined = $derived(profile.joinedGroups || {});
  let directory = $derived(
    (tab === 'joined' ? Object.values(joined).map((v) => v.group) : profile.groupDirectory || [])
      .filter(
        (g) =>
          (tab === 'joined' || (tab === 'local' ? !!g.locality : !g.locality)) &&
          (tab !== 'local' || !profile.groupLocality || (!!g.locality &&
            (['city','region','country'] as const).every(k => !profile.groupLocality?.[k] || g.locality![k].toLowerCase()===profile.groupLocality[k].toLowerCase()))) &&
          [g.name, g.description, g.category, g.locality?.city, g.locality?.region, g.locality?.country]
            .join(' ')
            .toLowerCase()
            .includes(query.toLowerCase().trim()),
      )
      .sort((a, b) => a.name.localeCompare(b.name)),
  );
  const categories = $derived([...new Set(directory.map(group => group.category))].sort());
  const showCategories = $derived(tab !== 'joined' && directory.length >= 3 && categories.length > 1);
  const selectedCategory = $derived(showCategories && categories.includes(browseCategory) ? browseCategory : '');
  const groups = $derived(directory.filter(group => !selectedCategory || group.category === selectedCategory));
  let userChoseTab = $state(false), directoryLoading = $state(true), directoryFailed = $state(false);
  let mounted = false;
  const hasCachedGroups = $derived(!!profile.groupDirectory?.length || !!Object.keys(joined).length);
  const awaitingDirectory = $derived(!profile.groupDirectory?.length &&
    (tab !== 'joined' || (!userChoseTab && !profile.recoveredGroups?.length)));
  function setTab(next: string) {
    userChoseTab = true;
    tab = next;
    browseCategory = '';
  }
  function browseWorldwide() {
    query = '';
    setTab('world');
  }
  function showAvailableGroups() {
    if (userChoseTab || Object.keys(joined).length || profile.recoveredGroups?.length) return;
    const available = profile.groupDirectory || [];
    if (available.length) tab = available.some(group => group.locality) ? 'local' : 'world';
  }
  async function loadDirectory() {
    directoryLoading = true;
    directoryFailed = false;
    try {
      await client.listGroups();
      if (mounted) showAvailableGroups();
    } catch {
      directoryFailed = true;
    } finally {
      directoryLoading = false;
    }
  }
  onMount(() => {
    mounted = true;
    showAvailableGroups();
    void loadDirectory();
    return () => { mounted = false; };
  });
  async function choose(id: string) {
    busy = true;
    error = '';
    try {
      if (!joined[id]) await client.joinGroup(id);
      if (mounted) await openGroup(id);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function create(e: SubmitEvent, close: () => void) {
    e.preventDefault();
    busy = true;
    formError = '';
    const details: GroupDetails = {
      name: name.trim(),
      description: description.trim(),
      category,
      locality: local
        ? { city: city.trim(), region: region.trim(), country: country.toUpperCase().trim() }
        : null,
    };
    try {
      const id = await client.joinGroup(null, details);
      close();
      if (mounted) await openGroup(id);
    } catch (e) {
      formError = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if showingPrivate}
  <PrivateGroups {client} alias={profile.alias} onBack={() => showingPrivate = false} />
{:else}
<div class="heading">
  <h1>Groups</h1>
  <ActionButton variant="tonal" onclick={() => showingPrivate = true}>Private groups</ActionButton>
  <Sheet
    title="Create a group"
    label="New group"
    {busy}
    prepare={() => {
      name = '';
      description = '';
      category = topics[0];
      local = !!profile.groupLocality;
      city = profile.groupLocality?.city || '';
      region = profile.groupLocality?.region || '';
      country = profile.groupLocality?.country || '';
      formError = '';
    }}
  >
    {#snippet children(close)}<form id={formId} class="group-form" onsubmit={(e) => create(e, close)}>
        <label>Group name<input required maxlength="60" bind:value={name} disabled={busy} /></label>
        <label>Description<textarea rows="3" required maxlength="300" bind:value={description} disabled={busy}></textarea></label>
        <label>Topic<select bind:value={category} disabled={busy}>
          {#each topics as topic (topic)}<option value={topic}>{topic}</option>{/each}
        </select></label>
        <label class="local-checkbox"><input type="checkbox" bind:checked={local} disabled={busy}/> City or town group</label>
        {#if local}
          <label>City or town<input bind:value={city} required maxlength="60" disabled={busy}/></label>
          <div class="region-fields">
            <label>Region<input bind:value={region} maxlength="60" disabled={busy}/></label>
            <label>Country code<input bind:value={country} required pattern={'[A-Za-z]{2}'} maxlength="2" placeholder="US" disabled={busy}/></label>
          </div>
        {/if}
      </form>
    {/snippet}
    {#snippet actions()}
      <div class="group-actions">
        {#if formError}<p class="group-form-error" role="alert">{formError}</p>{/if}
        <ActionButton type="submit" form={formId} variant="filled" disabled={busy}>Create group</ActionButton>
      </div>
    {/snippet}
  </Sheet>
</div>
<Tabs.Root bind:value={() => tab, setTab}
  ><Tabs.List class="dm-tabs" aria-label="Group directory"
    ><Tabs.Trigger class="btn" value="joined">Joined</Tabs.Trigger><Tabs.Trigger class="btn" value="local"
      >Local</Tabs.Trigger
    ><Tabs.Trigger class="btn" value="world">Worldwide</Tabs.Trigger></Tabs.List
  >
  <label class="directory-search">
    <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/></svg>
    <input type="search" bind:this={searchInput} aria-label="Search groups" placeholder="Search by group or town" bind:value={query} oninput={() => browseCategory = ''} />
  </label>
  <div use:pageMotion={tabs.indexOf(tab)}>
  {#each tabs as value}<Tabs.Content {value}
      >{#if tab === value}
        {#if tab==='local'}
          <div class="town-filter">
          <Sheet label={profile.groupLocality ? townLabel(profile.groupLocality) : 'Choose a town'} title="Local groups" prepare={()=>{townQuery='';townError=''}}>
            {#snippet children(close)}
              <div class="town-search">
                <input type="search" aria-label="Search cities and towns" placeholder="Search cities and towns"
                  bind:value={townQuery} />
              </div>
              <div class="place-list">
                {#each towns as town}
                  <ActionButton type="button" onclick={() => selectTown(town, close)}>{townLabel(town)}</ActionButton>
                {:else}
                  <p role="status">No groups in matching towns.</p>
                {/each}
              </div>
              {#if townError}<p role="alert">{townError}</p>{/if}
              <div class="town-footer">
                <ActionButton variant="tonal" onclick={()=>selectTown(null,close)}>Browse all towns</ActionButton>
              </div>
            {/snippet}
          </Sheet>
          </div>
        {/if}
        {#if showCategories}
          <div class="category-options" role="group" aria-label="Category">
            {#each ['', ...categories] as option (option)}
              <button aria-pressed={selectedCategory === option} onclick={() => browseCategory = option}>{option || 'All'}</button>
            {/each}
          </div>
        {/if}
        {#if tab === 'joined' && !query && profile.recoveredGroups?.some((g) => !joined[g.id])}<h2>
            From your backup
          </h2>
          {#each profile.recoveredGroups.filter((g) => !joined[g.id]) as group (group.id)}<div
              class="group-row"
            >
              <span>{group.name}</span><ActionButton
                variant="tonal"
                disabled={busy}
                onclick={() => choose(group.id)}>Rejoin</ActionButton
              >
            </div>{/each}
        {/if}
        {#each groups as group (group.id)}
          <section class="directory-entry" aria-label={group.name}>
            <div class="place">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true">
                {#if group.locality}<path d="M20 10c0 5-8 12-8 12S4 15 4 10a8 8 0 1 1 16 0Z"/><circle cx="12" cy="10" r="2.5"/>
                {:else}<circle cx="12" cy="12" r="9"/><ellipse cx="12" cy="12" rx="4" ry="9"/><path d="M3 12h18"/>{/if}
              </svg>
              {group.locality ? townLabel(group.locality) : 'Worldwide'}
            </div>
            <div class="group-copy"><h2>{group.name}</h2><p>{group.description}</p>
              <span class="topic">{group.category}</span>
            </div>
            <ActionButton variant={joined[group.id] ? 'outlined' : 'tonal'} disabled={busy} onclick={() => choose(group.id)}>
              {joined[group.id] ? 'Open' : 'Join'}<AppIcon name={joined[group.id] ? 'arrowRight' : 'plus'}/>
            </ActionButton>
          </section>
        {:else}
          {#if awaitingDirectory && directoryLoading}
            <p class="directory-state" role="status">Loading groups…</p>
          {:else if !(awaitingDirectory && directoryFailed)}
            <div class="dm-empty">
              <h2>{query ? 'No matching groups' : tab === 'joined' ? 'No joined groups yet' : 'No groups here yet'}</h2>
              {#if query}<ActionButton onclick={() => { query = ''; browseCategory = ''; searchInput?.focus(); }}>Clear search</ActionButton>{/if}
              {#if tab !== 'world' && !query}<ActionButton onclick={browseWorldwide}>Browse worldwide groups</ActionButton>{/if}
            </div>
          {/if}
        {/each}
        {#if tab === 'local' && !query && directory.length > 0 && directory.length < 3}
          <ActionButton class="worldwide-link" onclick={browseWorldwide}>Browse worldwide groups</ActionButton>
        {/if}
      {/if}</Tabs.Content
    >{/each}
  </div></Tabs.Root
>
{#if directoryFailed}
  <div class="directory-state">
    <p role="status">{hasCachedGroups ? 'Showing saved groups. Reconnect to update the directory.' : 'Couldn’t load the group directory.'}</p>
    <ActionButton variant="outlined" onclick={loadDirectory}>Try again</ActionButton>
  </div>
{/if}
{#if error}<p role="status">{error}</p>{/if}

{/if}

<style>
  .directory-state { margin: 1rem 0; color: var(--quiet); font: var(--type-small); }
  .directory-state p { margin: 0 0 .5rem; }
  .group-form {
    display: grid;
    gap: 1rem;
    margin-block-start: .75rem;
  }

  .group-form label { display: grid; gap: .4rem; font: var(--type-label); }
  .group-form :is(input,select,textarea) { margin: 0; }
  .group-form .local-checkbox { display: flex; align-items: center; gap: .6rem; min-height: 44px; cursor: pointer; }
  .local-checkbox input { width: 1.2rem; height: 1.2rem; min-height: 0; accent-color: var(--plum); }
  .region-fields { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: .75rem; }
  .directory-search { display: flex; position: relative; align-items: center; margin-top: 1rem; }
  .directory-search svg { position: absolute; left: .875rem; color: var(--quiet); pointer-events: none; }
  .directory-search input {
    appearance: none;
    width: 100%;
    min-height: 2.875rem;
    margin: 0;
    padding: .625rem .875rem .625rem 2.65rem;
    border: 1px solid var(--line);
    border-radius: .65rem;
    background: var(--mtrl-sys-color-surface-container-lowest);
    color: var(--ink);
    font: var(--type-body);
  }
  .directory-search input::placeholder { color: var(--quiet); opacity: 1; }
  .directory-search input:focus-visible { outline: 2px solid var(--plum); outline-offset: 1px; }
  .category-options { display: flex; flex-wrap: wrap; gap: .375rem; margin-block: .5rem; }
  .category-options button { color: var(--quiet); padding: .45rem .65rem; min-height: 44px; border-radius: .5rem; }
  .category-options button[aria-pressed=true] { background: var(--accent); color: var(--ink); }
  .directory-entry { display: grid; grid-template-columns: 1fr auto; gap: .45rem 1rem; padding-block: 1.25rem; border-block-end: 1px solid var(--line); }
  .place { display: flex; align-items: center; gap: .4rem; grid-column: 1/-1; color: var(--plum); font: var(--type-label); overflow-wrap: anywhere; }
  .place svg { flex: none; }
  .group-copy { min-width: 0; overflow-wrap: anywhere; }
  .group-copy h2 { margin: 0; font: var(--type-post-title); }
  .group-copy p { margin: .4rem 0; line-height: 1.5; }
  .group-copy .topic { color: var(--quiet); font: var(--type-small); }
  .directory-entry :global(.btn) { align-self: start; }
  .directory-entry :global(.btn svg) { width: 16px; height: 16px; }
  @media (max-width: 24rem) { .directory-entry { gap: .5rem; } .directory-entry :global(.btn svg) { display: none; } }

  .group-form-error {
    margin: 0 auto 0 0;
    color: var(--mtrl-sys-color-error);
  }

  .group-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: .5rem;
  }

  .local-checkbox, .town-footer {
    display: flex;
  }

  .town-filter {
    margin-block: .75rem .25rem;
  }

  .town-search, .town-footer {
    margin-block-start: .75rem;
  }

  .town-search {
    margin-block-end: .75rem;
  }

  .place-list {
    scrollbar-width: thin;
    scrollbar-color: var(--mtrl-sys-color-outline) transparent;
  }
</style>
