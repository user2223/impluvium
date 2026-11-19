-- SPDX-FileCopyrightText: 2026 the impluvium contributors
-- SPDX-License-Identifier: AGPL-3.0-only

-- Infrastructure schema of a server instance: identity, binding to the public
-- content key, ticket key generations and the state of the first
-- initialisation. It holds no game, user, object or job data.
--
-- Conventions: every column is NOT NULL unless a null carries meaning; points in
-- time are timestamptz; every constraint is named <table>_<kind>[_<detail>] with
-- kind pk (primary key), uq (unique), ck (check). Keys are stored as raw bytes:
-- a key id is 8 bytes, a public key is 32 bytes (Ed25519).

-- The identity of this instance and the layout version of its data. One row.
CREATE TABLE instance_identity (
    singleton      boolean     NOT NULL DEFAULT true,
    instance_id    uuid        NOT NULL DEFAULT gen_random_uuid(),
    layout_version integer     NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT instance_identity_pk PRIMARY KEY (singleton),
    CONSTRAINT instance_identity_ck_singleton CHECK (singleton),
    CONSTRAINT instance_identity_ck_layout_version CHECK (layout_version >= 1)
);

-- The public content key this instance is bound to. A binding is retired, never
-- deleted; at most one binding is active (retired_at is null) at any time.
CREATE TABLE content_key_binding (
    binding_id bigint      GENERATED ALWAYS AS IDENTITY,
    key_id     bytea       NOT NULL,
    public_key bytea       NOT NULL,
    algorithm  text        NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    retired_at timestamptz,
    CONSTRAINT content_key_binding_pk PRIMARY KEY (binding_id),
    CONSTRAINT content_key_binding_uq_public_key UNIQUE (public_key),
    CONSTRAINT content_key_binding_ck_key_id CHECK (octet_length(key_id) = 8),
    CONSTRAINT content_key_binding_ck_public_key CHECK (octet_length(public_key) = 32),
    CONSTRAINT content_key_binding_ck_algorithm CHECK (algorithm IN ('ed25519')),
    CONSTRAINT content_key_binding_ck_retired CHECK (retired_at IS NULL OR retired_at >= created_at)
);

CREATE UNIQUE INDEX content_key_binding_uq_active
    ON content_key_binding ((true))
    WHERE retired_at IS NULL;

-- Generations of the ticket signing key and the time each one takes over. The
-- current generation is the highest one whose switch time has passed.
CREATE TABLE ticket_key_generation (
    generation  bigint      NOT NULL,
    key_id      bytea       NOT NULL,
    switch_at   timestamptz NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT ticket_key_generation_pk PRIMARY KEY (generation),
    CONSTRAINT ticket_key_generation_uq_switch_at UNIQUE (switch_at),
    CONSTRAINT ticket_key_generation_ck_generation CHECK (generation BETWEEN 1 AND 4294967295),
    CONSTRAINT ticket_key_generation_ck_key_id CHECK (octet_length(key_id) = 8)
);

-- Progress of the first initialisation, so an interrupted start is recognised
-- and never mistaken for a finished one. One row.
CREATE TABLE instance_initialisation (
    singleton    boolean     NOT NULL DEFAULT true,
    state        text        NOT NULL,
    started_at   timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    CONSTRAINT instance_initialisation_pk PRIMARY KEY (singleton),
    CONSTRAINT instance_initialisation_ck_singleton CHECK (singleton),
    CONSTRAINT instance_initialisation_ck_state CHECK (state IN ('started', 'completed')),
    CONSTRAINT instance_initialisation_ck_completed CHECK ((state = 'completed') = (completed_at IS NOT NULL)),
    CONSTRAINT instance_initialisation_ck_order CHECK (completed_at IS NULL OR completed_at >= started_at)
);
