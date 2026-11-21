//! The board: what the reads serve, in memory, rebuilt from the store at start and replaced
//! whole after each stored round. Reads clone an `Arc` and never touch the database.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, PoisonError, RwLock};

use time::OffsetDateTime;
use yieldagent_data::Decimal;
use yieldagent_data::defillama::SOURCE;
use yieldagent_data::screen::AVERAGE_WINDOW;

use crate::store::{self, LastRound, Outcome, Round, Stored};

/// What the board holds for one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceView {
    pub last: LastRound,
    /// When the last good round was observed; `None` if there never was one.
    pub last_good_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Default)]
pub struct View {
    pub sources: BTreeMap<String, SourceView>,
    /// The markets of each source's last good round: never invented, never carried from a
    /// failed round. Their age is judged at read time.
    pub markets: Vec<Stored>,
    /// The last [`AVERAGE_WINDOW`] unflagged base APYs of each pool the board shows, oldest
    /// first: the spike check's baseline. A pool gone from its source's last good round
    /// leaves it, so the map is bounded by the markets one round can hold.
    pub recent: BTreeMap<String, VecDeque<Decimal>>,
}

#[derive(Debug, Default)]
pub struct Board(RwLock<Arc<View>>);

impl Board {
    pub fn view(&self) -> Arc<View> {
        Arc::clone(&self.0.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn replace(&self, view: View) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(view);
    }

    /// Rebuilds the board from the store (start, and after a crash).
    pub async fn load(&self, pool: &sqlx::PgPool) -> Result<(), yieldagent_db::DbError> {
        let (rounds, good, markets) = store::latest(pool, &[SOURCE]).await?;
        let shown: Vec<&str> = markets.iter().map(|m| m.pool_id.as_str()).collect();
        let recent = store::recent_apys(pool, &shown, AVERAGE_WINDOW).await?;
        let good: BTreeMap<String, OffsetDateTime> = good.into_iter().collect();
        let sources = rounds
            .into_iter()
            .map(|last| {
                let last_good_at = good.get(&last.source).copied();
                (last.source.clone(), SourceView { last, last_good_at })
            })
            .collect();
        self.replace(View {
            sources,
            markets,
            recent: recent.into_iter().map(|(k, v)| (k, v.into())).collect(),
        });
        Ok(())
    }

    /// Applies a stored round.
    pub fn apply(&self, round: &Round) {
        let old = self.view();
        let mut view = View::clone(&old);
        let refusals = round
            .refusals
            .iter()
            .map(|(k, v)| ((*k).to_owned(), *v))
            .collect();
        let (ok, error_code, accepted) = match &round.outcome {
            Outcome::Ok(s) => (true, None, s.len()),
            Outcome::Down(code) => (false, Some((*code).to_owned()), 0),
        };
        let previous_good = view.sources.get(round.source).and_then(|s| s.last_good_at);
        view.sources.insert(
            round.source.to_owned(),
            SourceView {
                last: LastRound {
                    source: round.source.to_owned(),
                    observed_at: round.observed_at,
                    ok,
                    error_code,
                    entries: round.entries,
                    selected: round.selected,
                    accepted: u32::try_from(accepted).unwrap_or(u32::MAX),
                    refusals,
                },
                last_good_at: if ok {
                    Some(round.observed_at)
                } else {
                    previous_good
                },
            },
        );
        if let Outcome::Ok(snapshots) = &round.outcome {
            view.markets.retain(|m| m.source != round.source);
            view.recent
                .retain(|id, _| snapshots.iter().any(|s| s.market.pool_id == *id)); // LIMIT(recent-eviction)
            for s in snapshots {
                let recent = view.recent.entry(s.market.pool_id.clone()).or_default();
                // LIMIT(spike-baseline)
                if s.flags.is_empty() {
                    recent.push_back(s.market.apy_base);
                }
                while recent.len() > AVERAGE_WINDOW {
                    recent.pop_front();
                }
                view.markets.push(Stored {
                    round_id: round.id,
                    source: round.source.to_owned(),
                    pool_id: s.market.pool_id.clone(),
                    protocol: s.market.protocol.to_owned(),
                    symbol: s.market.symbol.clone(),
                    apy: s.market.apy,
                    apy_base: s.market.apy_base,
                    apy_reward: s.market.apy_reward,
                    tvl_usd: s.market.tvl_usd,
                    observed_at: round.observed_at,
                    flags: s.flags.iter().map(|f| f.code().to_owned()).collect(),
                    cross_check: s.cross_check.code().to_owned(),
                });
            }
            view.markets
                .sort_by(|a, b| (&a.source, &a.pool_id).cmp(&(&b.source, &b.pool_id)));
        }
        self.replace(view);
    }
}
