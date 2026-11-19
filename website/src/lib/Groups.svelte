<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import { onMount } from 'svelte';
  import { Tabs } from 'bits-ui';
  import { Checkbox, Chip, Chips, Search, Select, SelectOption, TextField } from 'material/svelte';
  import Sheet from './Sheet.svelte';
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
  const formId = $props.id();
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
  function setTab(next: string) {
    tab = next;
    browseCategory = '';
  }
  function browseWorldwide() {
    query = '';
    setTab('world');
  }
  onMount(() => {
    client
      .listGroups()
      .catch(() => (error = 'Showing saved groups. Reconnect to update the directory.'));

  });
  async function choose(id: string) {
    busy = true;
    error = '';
    try {
      if (!joined[id]) await client.joinGroup(id);
      await openGroup(id);
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
      await openGroup(id);
    } catch (e) {
      formError = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="heading">
  <h1>Groups</h1>
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
        <TextField variant="outlined" label="Group name" bind:value={name} required maxlength={60} disabled={busy} />
        <TextField variant="outlined" type="multiline" label="Description" bind:value={description}
          required maxlength={300} disabled={busy} />
        <Select variant="outlined" label="Topic" bind:value={category} disabled={busy}>
          {#each topics as topic (topic)}
            <SelectOption value={topic}>{topic}</SelectOption>
          {/each}
        </Select>
        <div class="local-checkbox">
          <Checkbox bind:checked={local} disabled={busy}>City or town group</Checkbox>
        </div>
        {#if local}
          <TextField variant="outlined" label="City or town" aria-label="City or town"
            bind:value={city} required maxlength={60} disabled={busy} />
          <TextField variant="outlined" label="Region" bind:value={region} maxlength={60} disabled={busy} />
          <TextField variant="outlined" label="Country code" bind:value={country} required
            pattern={'[A-Za-z]{2}'} maxlength={2} placeholder="US" disabled={busy} />
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
  ><Tabs.List class="dm-tabs mtrl-tabs mtrl-tabs--primary" aria-label="Group directory"
    ><Tabs.Trigger class="mtrl-button mtrl-tab mtrl-tab--text-only" value="joined">Joined</Tabs.Trigger><Tabs.Trigger class="mtrl-button mtrl-tab mtrl-tab--text-only" value="local"
      >Local</Tabs.Trigger
    ><Tabs.Trigger class="mtrl-button mtrl-tab mtrl-tab--text-only" value="world">Worldwide</Tabs.Trigger></Tabs.List
  >
  <Search
    class="directory-search"
    aria-label="Search groups"
    placeholder="Search by group or town"
    noExpandOnFocus
    fullWidth
    value={query}
    oninput={({ detail }) => { query = detail.value; browseCategory = ''; }}
  />
  {#each ['joined', 'local', 'world'] as value}<Tabs.Content {value}
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
          <Chips class="group-categories" label="Category" selection="single" selectionRequired value={selectedCategory || 'all'}
            onchange={({ detail }) => { browseCategory = typeof detail.value === 'string' ? detail.value : ''; }}>
            <Chip value="all" variant="filter">All</Chip>
            {#each categories as value (value)}<Chip {value} variant="filter">{value}</Chip>{/each}
          </Chips>
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
        {#each groups as group (group.id)}<section class="group-row">
            <div>
              <h2>{group.name}</h2>
              <p>{group.description}</p>
              <small
                >{group.locality
                  ? [group.locality.city, group.locality.region, group.locality.country]
                      .filter(Boolean)
                      .join(', ')
                  : 'Worldwide'} · {group.category}</small
              >
            </div>
            <ActionButton variant="tonal" disabled={busy} onclick={() => choose(group.id)}
              >{joined[group.id] ? 'Open' : 'Join'}</ActionButton
            >
          </section>
        {:else}<div class="dm-empty">
            <h2>
              {query
                ? 'No matching groups'
                : tab === 'joined'
                  ? 'No joined groups yet'
                  : 'No groups here yet'}
            </h2>
            {#if tab !== 'world' && !query}<ActionButton onclick={browseWorldwide}>Browse worldwide groups</ActionButton>{/if}
          </div>{/each}
        {#if tab === 'local' && !query && directory.length > 0 && directory.length < 3}
          <ActionButton class="worldwide-link" onclick={browseWorldwide}>Browse worldwide groups</ActionButton>
        {/if}
      {/if}</Tabs.Content
    >{/each}</Tabs.Root
>
{#if error}<p role="status">{error}</p>{/if}

<style>
  .group-form {
    display: grid;
    gap: 1rem;
    margin-block-start: .75rem;
  }

  /* Keep the floating-label motion while switching theme colors immediately. */
  .group-form :global(:is(m-text-field, m-select))::part(label) {
    transition-property: transform, left;
  }

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

  /* Switch the search surface and foreground together with the device theme. */
  :global(m-search.directory-search)::part(container) {
    transition-property: border-radius, height;
  }

  .place-list {
    scrollbar-width: thin;
    scrollbar-color: var(--mtrl-sys-color-outline) transparent;
  }
</style>
