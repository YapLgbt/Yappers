<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import { ToggleGroup } from '@skeletonlabs/skeleton-svelte';
  import AppIcon from './AppIcon.svelte';
  import Avatar from './Avatar.svelte';
  import { blankProfile, PROFILE_PHOTO_LENGTH, PROFILE_PHOTO_SIZE, type Community, type Profile } from './community';
  import { onDestroy, tick } from 'svelte';
  let { client, profile }: { client: Community; profile: Profile } = $props();
  const intentions = [
    'Friends',
    'Dates',
    'Community',
    'Creative pals',
    'Chosen family',
    'Just yapping',
  ];
  const cropInset = 0.08;
  const cropFraction = 1 - 2 * cropInset;
  const maxZoom = 4;
  const directions = [
    { label: 'Move photo left', icon: 'arrowLeft', x: -1, y: 0 },
    { label: 'Move photo up', icon: 'arrowUp', x: 0, y: -1 },
    { label: 'Move photo down', icon: 'arrowDown', x: 0, y: 1 },
    { label: 'Move photo right', icon: 'arrowRight', x: 1, y: 0 },
  ] as const;
  let alias = $state(''),
    draft = $state(blankProfile()),
    error = $state(''),
    busy = $state(false),
    loadingPhoto = $state(false);
  let source = $state.raw<ImageBitmap | null>(null),
    canvas = $state<HTMLCanvasElement>(),
    zoom = $state(1),
    x = $state(0.5),
    y = $state(0.5),
    version = 0, expectedRevision = 0;
  let initial = $state(''), editing = $state(false);
  let closing = $state(false), reserveScrollbar = $state(false);
  let dialog: HTMLDialogElement;
  let photoInput: HTMLInputElement;
  let trigger: HTMLButtonElement;
  const formId = $props.id();
  const options = $derived([...new Set([...intentions, ...(profile.publicProfile?.hereFor ?? [])])]);
  const snapshot = () => JSON.stringify([alias, { ...draft, hereFor: [...draft.hereFor].sort() }]);
  const dirty = $derived(!!source || snapshot() !== initial);
  const cropSide = $derived(source ? Math.min(source.width, source.height) / zoom : 0);
  const cropLocked = $derived(busy || loadingPhoto);
  const clamp = (value: number) => Math.max(0, Math.min(1, value));

  function resetCrop() {
    zoom = 1;
    x = y = 0.5;
  }

  function zoomBy(step: number) {
    zoom = Math.max(1, Math.min(maxZoom, zoom + step));
  }

  function movePhoto(dx: number, dy: number) {
    if (!source || cropLocked) return;
    const travelX = source.width - cropSide;
    const travelY = source.height - cropSide;
    if (dx && travelX) x = clamp(x - dx * cropSide * 0.05 / travelX);
    if (dy && travelY) y = clamp(y - dy * cropSide * 0.05 / travelY);
  }

  function cancel(event: Event) {
    event.preventDefault();
    if (busy || closing || (dirty && !window.confirm('Discard your unsaved profile changes?'))) return;
    void dismiss();
  }

  async function dismiss() {
    if (closing || !dialog.open) return;
    closing = true;
    await tick();
    // Keep focus and the backdrop in the modal until the surface has finished leaving.
    await Promise.allSettled(dialog.getAnimations().map(animation => animation.finished));
    if (dialog.open) dialog.close();
  }

  function didClose() {
    if (dialog.open) return;
    editing = closing = false;
    clearImage();
    trigger?.focus({ preventScroll: true });
  }
  $effect(()=>{
    if (!editing || !dirty) return;
    const warn=(event:BeforeUnloadEvent)=>event.preventDefault();
    window.addEventListener('beforeunload',warn);
    return ()=>window.removeEventListener('beforeunload',warn);
  });
  function clearImage() {
    version++;
    source?.close();
    source = null;
    loadingPhoto = false;
  }
  onDestroy(clearImage);
  async function prepare(event: MouseEvent & { currentTarget: HTMLButtonElement }) {
    if (editing) return;
    trigger = event.currentTarget;
    clearImage();
    alias = profile.alias;
    expectedRevision = profile.profileRevision || 0;
    draft = {
      ...blankProfile(),
      ...profile.publicProfile,
      hereFor: [...(profile.publicProfile?.hereFor || [])],
    };
    initial = snapshot();
    reserveScrollbar = window.innerWidth > document.documentElement.clientWidth;
    editing = true;
    closing = false;
    error = '';
    await tick();
    dialog.showModal();
    dialog.querySelector('form')?.scrollTo(0, 0);
  }
  async function pick(file?: File) {
    if (!file) return;
    const ticket = ++version;
    loadingPhoto = true;
    error = '';
    try {
      if (file.size > 20 * 1024 * 1024) throw Error('Choose a photo smaller than 20 MB.');
      const image = await createImageBitmap(file);
      if (ticket !== version) {
        image.close();
        return;
      }
      source?.close();
      source = image;
      resetCrop();
    } catch (e) {
      if (ticket === version) error = e instanceof Error ? e.message : 'Couldn’t open this photo.';
    } finally {
      if (ticket === version) loadingPhoto = false;
    }
  }
  type Point = { x: number; y: number };
  function gestures(node: HTMLCanvasElement) {
    const pointers = new Map<number, Point>();
    function point(event: PointerEvent) {
      const rect = node.getBoundingClientRect();
      return {
        x: ((event.clientX - rect.left) / rect.width - cropInset) / cropFraction,
        y: ((event.clientY - rect.top) / rect.height - cropInset) / cropFraction,
      };
    }
    function frame() {
      const [a, b] = pointers.values();
      return b
        ? { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, span: Math.hypot(a.x - b.x, a.y - b.y) }
        : { ...a, span: 0 };
    }
    function down(event: PointerEvent) {
      if (cropLocked || event.button !== 0 || pointers.size >= 2) return;
      pointers.set(event.pointerId, point(event));
      node.setPointerCapture(event.pointerId);
    }
    function move(event: PointerEvent) {
      if (cropLocked || !source || !pointers.has(event.pointerId)) return;
      const old = frame();
      pointers.set(event.pointerId, point(event));
      const next = frame();
      const before = cropSide;
      const anchorX = (source.width - before) * x + old.x * before;
      const anchorY = (source.height - before) * y + old.y * before;
      if (old.span > 0 && next.span > 0)
        zoom = Math.max(1, Math.min(maxZoom, zoom * next.span / old.span));
      const after = cropSide;
      x = source.width === after ? 0.5 : clamp((anchorX - next.x * after) / (source.width - after));
      y = source.height === after ? 0.5 : clamp((anchorY - next.y * after) / (source.height - after));
    }
    function end(event: PointerEvent) {
      pointers.delete(event.pointerId);
    }
    const listeners = new AbortController();
    const options = { signal: listeners.signal };
    node.addEventListener('pointerdown', down, options);
    node.addEventListener('pointermove', move, options);
    for (const type of ['pointerup', 'pointercancel', 'lostpointercapture'] as const)
      node.addEventListener(type, end, options);
    return {
      destroy() {
        listeners.abort();
        pointers.clear();
      },
    };
  }
  function paint(target: HTMLCanvasElement, img: ImageBitmap, inset = 0) {
    const context = target.getContext('2d')!;
    const side = Math.min(img.width, img.height) / zoom;
    const scale = target.width * (1 - 2 * inset) / side;
    context.clearRect(0, 0, target.width, target.height);
    context.drawImage(
      img,
      inset * target.width - (img.width - side) * x * scale,
      inset * target.height - (img.height - side) * y * scale,
      img.width * scale,
      img.height * scale,
    );
  }
  $effect(() => {
    if (canvas && source) paint(canvas, source, cropInset);
  });
  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (busy || closing || loadingPhoto || !dirty) return;
    busy = true;
    error = '';
    try {
      if (alias.trim().length < 2)
        throw Error('Use at least two characters for your display name.');
      let photo = draft.photo;
      if (source) {
        const out = document.createElement('canvas');
        out.width = out.height = PROFILE_PHOTO_SIZE;
        paint(out, source);
        for (const quality of [0.87, 0.78, 0.68, 0.58, 0.48, 0.38, 0.28, 0.18, 0.08]) {
          photo = out.toDataURL('image/jpeg', quality).split(',')[1];
          if (photo.length <= PROFILE_PHOTO_LENGTH) break;
        }
        if (photo.length > PROFILE_PHOTO_LENGTH) throw Error('Couldn’t resize this photo. Choose another.');
      }
      const updated = { ...draft, photo, hereFor: [...draft.hereFor] };
      await client.setPublicProfile(alias.trim(), updated, expectedRevision);
      await dismiss();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Couldn’t save your profile.';
    } finally {
      busy = false;
    }
  }
</script>

<ActionButton type="button" variant="filled" aria-label="Edit profile" aria-haspopup="dialog" onclick={prepare}>Edit profile</ActionButton>
<dialog class="profile-editor" class:closing class:reserveScrollbar bind:this={dialog}
  aria-labelledby={formId + '-title'} oncancel={cancel} onclose={didClose}>
  <header><h2 id={formId + '-title'}>Edit profile</h2></header>
  <form id={formId} onsubmit={save}>
    <div class="profile-photo" aria-busy={loadingPhoto}>
      <button type="button" class="photo-picker" aria-label={source || draft.photo ? 'Change profile photo' : 'Add profile photo'}
        disabled={busy || loadingPhoto} onclick={() => photoInput.click()}>
        <Avatar photo={draft.photo} label={alias} large />
        <span class="photo-picker-icon"><AppIcon name="camera" /></span>
      </button>
      <input bind:this={photoInput} type="file" accept="image/*" aria-label="Profile photo" hidden
        onchange={(event) => {
          const input = event.currentTarget;
          void pick(input.files?.[0]);
          input.value = '';
        }} />
      <div>
        <h3>Profile photo</h3>
        {#if source || draft.photo}
          <ActionButton type="button" disabled={busy || loadingPhoto} onclick={() => {
            clearImage();
            draft.photo = '';
            error = '';
          }}>Remove photo</ActionButton>
        {:else}
          <p class="hint">Optional</p>
        {/if}
      </div>
    </div>

    {#if source}
      <section class="profile-crop" aria-labelledby={formId + '-crop'}>
        <h3 id={formId + '-crop'}>Crop photo</h3>
        <div class="profile-crop-layout">
          <figure class="profile-crop-figure">
            <div class="crop-stage" role="img" aria-label="Profile photo crop preview"
              style:--crop-inset={cropInset * 100 + '%'}>
              <canvas class="crop-preview" class:locked={cropLocked} width="448" height="448"
                bind:this={canvas} use:gestures aria-hidden="true"></canvas>
            </div>
            <figcaption>Drag to position.<span class="crop-touch-hint">{' Pinch to zoom.'}</span></figcaption>
          </figure>
          <div class="profile-crop-controls">
            <div class="crop-control-row" role="group" aria-label="Zoom">
              <button type="button" class="btn-icon crop-button preset-tonal-brand"
                aria-label="Zoom out"
                disabled={cropLocked || zoom <= 1} onclick={() => zoomBy(-0.25)}>
                <AppIcon name="minus" />
              </button>
              <span class="crop-zoom-value" aria-live="polite" aria-atomic="true">{Math.round(zoom * 100)}%</span>
              <button type="button" class="btn-icon crop-button preset-tonal-brand"
                aria-label="Zoom in"
                disabled={cropLocked || zoom >= maxZoom} onclick={() => zoomBy(0.25)}>
                <AppIcon name="plus" />
              </button>
            </div>
            <div class="crop-control-row" role="group" aria-label="Photo position">
              {#each directions as direction (direction.icon)}
                <button type="button" class="btn-icon crop-button preset-outlined-surface-500"
                  aria-label={direction.label}
                  disabled={cropLocked || (direction.x ? source.width : source.height) <= cropSide}
                  onclick={() => movePhoto(direction.x, direction.y)}>
                  <AppIcon name={direction.icon} />
                </button>
              {/each}
            </div>
            <ActionButton type="button" disabled={cropLocked} onclick={resetCrop}>Reset crop</ActionButton>
          </div>
        </div>
      </section>
    {/if}
    <label>Display name<input bind:value={alias} minlength="2" maxlength="30" required /></label>
    <label>Pronouns<input bind:value={draft.pronouns} maxlength="40" /></label>
    <label>Bio<textarea bind:value={draft.bio} maxlength="280"></textarea></label>
    <label>Identity<input bind:value={draft.identity} maxlength="80" /></label>
    <label>Interests<input bind:value={draft.interests} maxlength="120" /></label>
    <fieldset class="profile-intentions">
      <legend>Here for</legend>
      <ToggleGroup multiple value={draft.hereFor} disabled={busy} aria-label="Here for"
        onValueChange={({ value }) => draft.hereFor = value}>
        {#each options as tag (tag)}
          <ToggleGroup.Item class="chip" value={tag}>{tag}</ToggleGroup.Item>
        {/each}
      </ToggleGroup>
    </fieldset>
  </form>
  <footer>
    {#if error}<p class="profile-error" role="alert">{error}</p>{/if}
    <div class="profile-editor-actions">
      <ActionButton type="button" disabled={busy} onclick={cancel}>Cancel</ActionButton>
      <ActionButton type="submit" form={formId} variant="filled" disabled={busy || loadingPhoto || !dirty}>Save profile</ActionButton>
    </div>
  </footer>
</dialog>
