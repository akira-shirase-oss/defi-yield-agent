-- M1: the market data worker's rounds and snapshots. Append-only, like account_events:
-- the history is the evidence later milestones plan from, so nothing rewrites it.
--
-- One round per fetch, identified by the worker before it fetches; one transaction writes
-- the round and its snapshots, so a crash leaves the whole round or none of it, and a
-- retried write of the same round id changes nothing (ON CONFLICT DO NOTHING).
--
-- The CHECKs repeat the worker's validation (crates/yieldagent-data/src/market.rs): a bug
-- there cannot store a negative or implausible APY.
CREATE TABLE market_rounds (
    round_id uuid PRIMARY KEY,
    source text NOT NULL CHECK (source ~ '^[a-z0-9-]{1,32}$'),
    started_at timestamptz NOT NULL,
    observed_at timestamptz NOT NULL CHECK (observed_at >= started_at),
    outcome text NOT NULL CHECK (outcome IN ('ok', 'down')),
    error_code text CHECK (error_code ~ '^[a-z_]{1,64}$'),
    entries integer NOT NULL CHECK (entries >= 0),
    selected integer NOT NULL CHECK (selected >= 0),
    accepted integer NOT NULL CHECK (accepted >= 0),
    refusals jsonb NOT NULL CHECK (jsonb_typeof(refusals) = 'object'),
    CHECK ((outcome = 'ok') = (error_code IS NULL)),
    CHECK (outcome = 'ok' OR accepted = 0)
);
CREATE INDEX market_rounds_latest ON market_rounds (source, observed_at DESC);

CREATE TABLE market_snapshots (
    round_id uuid NOT NULL REFERENCES market_rounds (round_id),
    pool_id text NOT NULL CHECK (pool_id ~ '^[A-Za-z0-9-]{1,64}$'),
    source text NOT NULL CHECK (source ~ '^[a-z0-9-]{1,32}$'),
    chain text NOT NULL CHECK (chain = 'Base'),
    protocol text NOT NULL CHECK (protocol IN ('aave-v3', 'compound-v3', 'morpho-blue')),
    symbol text NOT NULL CHECK (symbol ~ '^[A-Za-z0-9.+_-]{1,32}$'),
    apy numeric NOT NULL CHECK (apy >= 0 AND apy <= 100),
    tvl_usd numeric NOT NULL CHECK (tvl_usd >= 0 AND tvl_usd <= 1000000000000),
    observed_at timestamptz NOT NULL,
    flags text[] NOT NULL CHECK (flags <@ ARRAY['apy_spike']::text[]),
    cross_check text NOT NULL CHECK (cross_check IN ('unavailable', 'agrees', 'disagrees')),
    PRIMARY KEY (round_id, pool_id)
);
CREATE INDEX market_snapshots_history ON market_snapshots (pool_id, observed_at DESC);

CREATE TRIGGER market_rounds_append_only BEFORE UPDATE OR DELETE ON market_rounds
    FOR EACH ROW EXECUTE FUNCTION refuse_change();
CREATE TRIGGER market_rounds_no_truncate BEFORE TRUNCATE ON market_rounds
    FOR EACH STATEMENT EXECUTE FUNCTION refuse_change();
CREATE TRIGGER market_snapshots_append_only BEFORE UPDATE OR DELETE ON market_snapshots
    FOR EACH ROW EXECUTE FUNCTION refuse_change();
CREATE TRIGGER market_snapshots_no_truncate BEFORE TRUNCATE ON market_snapshots
    FOR EACH STATEMENT EXECUTE FUNCTION refuse_change();
-- ALWAYS: session_replication_role = replica does not silence them either.
ALTER TABLE market_rounds ENABLE ALWAYS TRIGGER market_rounds_append_only;
ALTER TABLE market_rounds ENABLE ALWAYS TRIGGER market_rounds_no_truncate;
ALTER TABLE market_snapshots ENABLE ALWAYS TRIGGER market_snapshots_append_only;
ALTER TABLE market_snapshots ENABLE ALWAYS TRIGGER market_snapshots_no_truncate;

GRANT SELECT, INSERT ON market_rounds, market_snapshots TO yieldagent_app;
