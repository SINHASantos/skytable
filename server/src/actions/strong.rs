/*
 * Created on Mon Sep 21 2020
 *
 * This file is a part of Skytable
 * Skytable (formerly known as TerrabaseDB or Skybase) is a free and open-source
 * NoSQL database written by Sayan Nandan ("the Author") with the
 * vision to provide flexibility in data modelling without compromising
 * on performance, queryability or scalability.
 *
 * Copyright (c) 2020, Sayan Nandan <ohsayan@outlook.com>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 *
*/

//! # Strong Actions
//! Strong actions are like "do all" or "fail all" actions, built specifically for
//! multiple keys. So let's say you used `SSET` instead of `MSET` for setting keys:
//! what'd be the difference?
//! In this case, if all the keys are non-existing, which is a requirement for `MSET`,
//! only then would the keys be set. That is, only if all the keys can be set, will the action
//! run and return code `0` - otherwise the action won't do anything and return an overwrite error.
//! There is no point of using _strong actions_ for a single key/value pair, since it will only
//! slow things down due to the checks performed.
//! Do note that this isn't the same as the gurantees provided by ACID transactions

use crate::corestore::Data;
use crate::dbnet::connection::prelude::*;

action!(
    /// Run an `SSET` query
    ///
    /// This either returns `Okay` if all the keys were set, or it returns an
    /// `Overwrite Error` or code `2`
    fn sset(handle: &crate::corestore::Corestore, con: &mut T, mut act: ActionIter) {
        let howmany = act.len();
        if is_lowbit_set!(howmany) || howmany == 0 {
            return con.write_response(responses::groups::ACTION_ERR).await;
        }
        let mut_table = kve!(con, handle);
        let failed;
        {
            // This iterator gives us the keys and values, skipping the first argument which
            // is the action name
            let mut key_iter = act.as_ref().iter();
            let mut good_to_set = true;
            'outer: while let (Some(key), _) = (key_iter.next(), key_iter.next()) {
                // unwrap or true because that will make the operation fail
                if mut_table.exists(key.clone()).unwrap_or(true) {
                    good_to_set = false;
                    break 'outer;
                }
            }
            if registry::state_okay() {
                if good_to_set {
                    failed = Some(false);
                    // Since the failed flag is false, none of the keys existed
                    // So we can safely set the keys
                    while let (Some(key), Some(value)) = (act.next(), act.next()) {
                        let _ = mut_table.set(key.into(), value.into());
                    }
                } else {
                    failed = Some(true);
                }
            } else {
                failed = None;
            }
        }
        if let Some(failed) = failed {
            if failed {
                con.write_response(responses::groups::OVERWRITE_ERR).await
            } else {
                con.write_response(responses::groups::OKAY).await
            }
        } else {
            con.write_response(responses::groups::SERVER_ERR).await
        }
    }
);

action!(
    /// Run an `SDEL` query
    ///
    /// This either returns `Okay` if all the keys were `del`eted, or it returns a
    /// `Nil`, which is code `1`
    fn sdel(handle: &crate::corestore::Corestore, con: &mut T, act: ActionIter) {
        let howmany = act.len();
        if howmany == 0 {
            return con.write_response(responses::groups::ACTION_ERR).await;
        }
        let failed;
        {
            let mut key_iter = act.as_ref().iter();
            if registry::state_okay() {
                let mut_table = kve!(con, handle);
                if key_iter.all(|key| mut_table.exists(key.clone()).unwrap_or(false)) {
                    failed = Some(false);
                    // Since the failed flag is false, all of the keys exist
                    // So we can safely delete the keys
                    act.into_iter().for_each(|key| {
                        let _ = mut_table.remove(key);
                    });
                } else {
                    failed = Some(true);
                }
            } else {
                failed = None;
            }
        }
        if let Some(failed) = failed {
            if failed {
                con.write_response(responses::groups::NIL).await
            } else {
                con.write_response(responses::groups::OKAY).await
            }
        } else {
            con.write_response(responses::groups::SERVER_ERR).await
        }
    }
);

action!(
    /// Run an `SUPDATE` query
    ///
    /// This either returns `Okay` if all the keys were updated, or it returns `Nil`
    /// or code `1`
    fn supdate(handle: &crate::corestore::Corestore, con: &mut T, mut act: ActionIter) {
        let howmany = act.len();
        if is_lowbit_set!(howmany) || howmany == 0 {
            return con.write_response(responses::groups::ACTION_ERR).await;
        }
        let mut failed = Some(false);
        {
            let mut key_iter = act.as_ref().iter();
            if registry::state_okay() {
                let mut_table = kve!(con, handle);
                while let (Some(key), _) = (key_iter.next(), key_iter.next()) {
                    if !mut_table.exists(key.clone()).unwrap_or(false) {
                        // With one of the keys failing to exist - this action can't clearly be done
                        // So we'll set `failed` to true and ensure that we check this while
                        // writing a response back to the client
                        failed = Some(true);
                        break;
                    }
                }
                // clippy thinks we're doing something complex when we aren't, at all!
                #[allow(clippy::blocks_in_if_conditions)]
                if unsafe { !failed.unsafe_unwrap() } {
                    // Since the failed flag is false, none of the keys existed
                    // So we can safely update the keys
                    while let (Some(key), Some(value)) = (act.next(), act.next()) {
                        let _ = mut_table.update(Data::from(key), Data::from(value));
                    }
                }
            } else {
                failed = None;
            }
        }
        if let Some(failed) = failed {
            if failed {
                con.write_response(responses::groups::NIL).await
            } else {
                con.write_response(responses::groups::OKAY).await
            }
        } else {
            con.write_response(responses::groups::SERVER_ERR).await
        }
    }
);
