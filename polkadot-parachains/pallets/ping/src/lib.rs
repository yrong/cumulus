// Copyright 2020-2021 Parity Technologies (UK) Ltd.
// This file is part of Cumulus.

// Cumulus is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Cumulus is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Cumulus.  If not, see <http://www.gnu.org/licenses/>.

//! Pallet to spam the XCM/UMP.

#![cfg_attr(not(feature = "std"), no_std)]

use sp_std::prelude::*;
use sp_runtime::traits::Saturating;
use frame_system::Config as SystemConfig;
use cumulus_primitives_core::ParaId;
use cumulus_pallet_xcm::{Origin as CumulusOrigin, ensure_sibling_para};
use xcm::v0::{Xcm, Error as XcmError, SendXcm, OriginKind, MultiLocation, Junction};
pub use pallet::*;

use codec::{Decode, Encode};
use frame_support::sp_runtime::MultiSignature;

type BalanceOf = u128;

#[derive(Encode, Decode)]
pub enum RelayTemplatePalletCall {
	#[codec(index = 100)] // the index should match the position of the module in `construct_runtime!`
	DoSomething(DoSomethingCall),
}

#[derive(Encode, Decode)]
pub enum DoSomethingCall {
	#[codec(index = 0)] // the index should match the position of the dispatchable in the target pallet
	Something(u32),
}

#[derive(Encode, Decode)]
pub enum CrowdloanPalletCall {
	#[codec(index = 27)] // the index should match the position of the module in `construct_runtime!`
	CrowdloanContribute(ContributeCall),
}

#[derive(Debug, PartialEq, Encode, Decode)]
pub struct Contribution {
	#[codec(compact)]
	index: ParaId,
	#[codec(compact)]
	value: BalanceOf,
	signature: Option<MultiSignature>,
}

#[derive(Encode, Decode)]
pub enum ContributeCall {
	#[codec(index = 1)] // the index should match the position of the dispatchable in the target pallet
	Contribute(Contribution),
}

#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use super::*;
	use xcm::v0::{MultiAsset};
	use xcm::v0::opaque::Order;

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	/// The module configuration trait.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		type Origin: From<<Self as SystemConfig>::Origin> + Into<Result<CumulusOrigin, <Self as Config>::Origin>>;

		/// The overarching call type; we assume sibling chains use the same type.
		type Call: From<Call<Self>> + Encode;

		type XcmSender: SendXcm;

		type SelfParaId: Get<ParaId>;

		type AccountId: From<<Self as SystemConfig>::AccountId>;

		type SendXcmOrigin: EnsureOrigin<<Self as SystemConfig>::Origin, Success=MultiLocation>;

		type UnitWeightCost: Get<u64>;

	}

	/// The target parachains to ping.
	#[pallet::storage]
	pub(super) type Targets<T: Config> = StorageValue<
		_,
		Vec<(ParaId, Vec<u8>)>,
		ValueQuery,
	>;

	/// The total number of pings sent.
	#[pallet::storage]
	pub(super) type PingCount<T: Config> = StorageValue<
		_,
		u32,
		ValueQuery,
	>;

	/// The sent pings.
	#[pallet::storage]
	pub(super) type Pings<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		u32,
		T::BlockNumber,
		OptionQuery,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	#[pallet::metadata(T::BlockNumber = "BlockNumber")]
	pub enum Event<T: Config> {
		PingSent(ParaId, u32, Vec<u8>),
		Pinged(ParaId, u32, Vec<u8>),
		PongSent(ParaId, u32, Vec<u8>),
		Ponged(ParaId, u32, Vec<u8>, T::BlockNumber),
		ErrorSendingPing(XcmError, ParaId, u32, Vec<u8>),
		ErrorSendingPong(XcmError, ParaId, u32, Vec<u8>),
		UnknownPong(ParaId, u32, Vec<u8>),
		TestMsg(u32),
		ErrorSendingTest(),
	}

	#[pallet::error]
	pub enum Error<T> {
		PingError,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_finalize(
			n: T::BlockNumber,
		) {
			for (para, payload) in Targets::<T>::get().into_iter() {
				let seq = PingCount::<T>::mutate(|seq| { *seq += 1; *seq });
				match T::XcmSender::send_xcm(
					MultiLocation::X2(Junction::Parent, Junction::Parachain(para.into())),
					Xcm::Transact {
						origin_type: OriginKind::Native,
						require_weight_at_most: 1_000,
						call: <T as Config>::Call::from(Call::<T>::ping(seq, payload.clone())).encode().into(),
					},
				) {
					Ok(()) => {
						Pings::<T>::insert(seq, n);
						Self::deposit_event(Event::PingSent(para, seq, payload));
					},
					Err(e) => {
						Self::deposit_event(Event::ErrorSendingPing(e, para, seq, payload));
					}
				}
			}
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		fn start(origin: OriginFor<T>, para: ParaId, payload: Vec<u8>) -> DispatchResult {
			ensure_root(origin)?;
			Targets::<T>::mutate(|t| t.push((para, payload)));
			Ok(())
		}

		#[pallet::weight(0)]
		fn start_many(origin: OriginFor<T>, para: ParaId, count: u32, payload: Vec<u8>) -> DispatchResult {
			ensure_root(origin)?;
			for _ in 0..count {
				Targets::<T>::mutate(|t| t.push((para, payload.clone())));
			}
			Ok(())
		}

		#[pallet::weight(0)]
		fn stop(origin: OriginFor<T>, para: ParaId) -> DispatchResult {
			ensure_root(origin)?;
			Targets::<T>::mutate(|t| if let Some(p) = t.iter().position(|(p, _)| p == &para) { t.swap_remove(p); });
			Ok(())
		}

		#[pallet::weight(0)]
		fn stop_all(origin: OriginFor<T>, maybe_para: Option<ParaId>) -> DispatchResult {
			ensure_root(origin)?;
			if let Some(para) = maybe_para {
				Targets::<T>::mutate(|t| t.retain(|&(x, _)| x != para));
			} else {
				Targets::<T>::kill();
			}
			Ok(())
		}

		#[pallet::weight(0)]
		fn ping(origin: OriginFor<T>, seq: u32, payload: Vec<u8>) -> DispatchResult {
			// Only accept pings from other chains.
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin))?;

			Self::deposit_event(Event::Pinged(para, seq, payload.clone()));
			match T::XcmSender::send_xcm(
				MultiLocation::X2(Junction::Parent, Junction::Parachain(para.into())),
				Xcm::Transact {
					origin_type: OriginKind::Native,
					require_weight_at_most: 1_000,
					call: <T as Config>::Call::from(Call::<T>::pong(seq, payload.clone())).encode().into(),
				},
			) {
				Ok(()) => Self::deposit_event(Event::PongSent(para, seq, payload)),
				Err(e) => Self::deposit_event(Event::ErrorSendingPong(e, para, seq, payload)),
			}
			Ok(())
		}

		#[pallet::weight(0)]
		fn pong(origin: OriginFor<T>, seq: u32, payload: Vec<u8>) -> DispatchResult {
			// Only accept pings from other chains.
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin))?;

			if let Some(sent_at) = Pings::<T>::take(seq) {
				Self::deposit_event(Event::Ponged(para, seq, payload, frame_system::Pallet::<T>::block_number().saturating_sub(sent_at)));
			} else {
				// Pong received for a ping we apparently didn't send?!
				Self::deposit_event(Event::UnknownPong(para, seq, payload));
			}
			Ok(())
		}

		#[pallet::weight(0)]
		fn test_upward_test_call(origin: OriginFor<T>, some_value: u32) -> DispatchResult {
			let who = ensure_signed(origin)?;

			let call = RelayTemplatePalletCall::DoSomething(DoSomethingCall::Something(some_value)).encode();

			let msg = Xcm::Transact {
				origin_type: OriginKind::SovereignAccount,
				require_weight_at_most: u64::MAX,
				call: call.into(),
			};

			log::info!(
				target: "ping",
				"Test pallet transact from {:?} as {:?}",
				who,
				msg,
			);

			match T::XcmSender::send_xcm(MultiLocation::X1(Junction::Parent), msg) {
				Ok(()) => {
					Self::deposit_event(Event::TestMsg(some_value));
					log::info!(
						target: "ping",
						"test transact sent success!"
					);
				},
				Err(e) => {
					Self::deposit_event(Event::ErrorSendingTest());
					log::error!(
						target: "ping",
						"test transact sent failed:{:?}",
						e,
					);
				}
			}
			Ok(())
		}


		#[pallet::weight(0)]
		fn test_upward_crowdloan_contribute(origin: OriginFor<T>, #[pallet::compact] value: BalanceOf) -> DispatchResult {
			let _who = ensure_signed(origin)?;

			let para_id = T::SelfParaId::get();

			let contribution = Contribution{ index: para_id, value, signature: None };

			let call = CrowdloanPalletCall::CrowdloanContribute(ContributeCall::Contribute(contribution)).encode();

			let msg = Xcm::Transact {
				origin_type: OriginKind::SovereignAccount,
				require_weight_at_most: u64::MAX,
				call: call.into(),
			};

			log::info!(
				target: "ping",
				"crowdloan transact from {:?} as {:?}",
				para_id,
				msg,
			);

			match T::XcmSender::send_xcm(MultiLocation::X1(Junction::Parent), msg) {
				Ok(()) => {
					Self::deposit_event(Event::TestMsg(0));
					log::info!(
						target: "ping",
						"crowdloan transact sent success!"
					);
				},
				Err(e) => {
					Self::deposit_event(Event::ErrorSendingTest());
					log::error!(
						target: "ping",
						"crowdloan transact sent failed:{:?}",
						e,
					);
				}
			}
			Ok(())
		}

		#[pallet::weight(0)]
		fn test_upward_transfer(origin: OriginFor<T>, some_value: u128) -> DispatchResult {

			let origin_location:MultiLocation = T::SendXcmOrigin::ensure_origin(origin)?;

			let account_location = origin_location.first().ok_or_else(|| Error::<T>::PingError)?;

			let account_32 = &*(account_location);

			let debt = T::UnitWeightCost::get() * 10;

			let msg = Xcm::ReserveAssetDeposit {
				assets:vec![MultiAsset::ConcreteFungible { id: MultiLocation::Null, amount: some_value }],
				effects: vec![
					Order::BuyExecution {
						fees: MultiAsset::All,
						weight: 0,
						debt,
						halt_on_error: false,
						xcm: vec![]
					},
					Order::DepositAsset {
						assets: vec![MultiAsset::All],
						dest: MultiLocation::X1(account_32.clone()),
					}
				]
			};

			log::info!(
				target: "ping",
				"upward transfer {:?}",
				msg,
			);

			match T::XcmSender::send_xcm(MultiLocation::X1(Junction::Parent), msg) {
				Ok(()) => {
					Self::deposit_event(Event::TestMsg(some_value as u32));
					log::info!(
						target: "ping",
						"upward transfer sent success!"
					);
				},
				Err(e) => {
					Self::deposit_event(Event::ErrorSendingTest());
					log::error!(
						target: "ping",
						"upward transfer sent failed:{:?}",
						e,
					);
				}
			}
			Ok(())
		}


		#[pallet::weight(0)]
		fn test_upward_withdraw_teleport_downward_transfer(origin: OriginFor<T>, some_value: u128) -> DispatchResult {

			let origin_location:MultiLocation = T::SendXcmOrigin::ensure_origin(origin)?;
			
			let account_location = origin_location.first().ok_or_else(|| Error::<T>::PingError)?;

			let account_32 = &*(account_location);

			let para_id = T::SelfParaId::get().into();

			let debt = T::UnitWeightCost::get() * 4;
			
			let msg = Xcm::WithdrawAsset {
				assets:vec![MultiAsset::ConcreteFungible { id: MultiLocation::Null, amount: some_value }],
				effects: vec![
					Order::BuyExecution {
						fees: MultiAsset::All,
						weight: 0,
						debt,
						halt_on_error: false,
						xcm: vec![]
					},
					Order::InitiateTeleport {
						assets: vec![MultiAsset::All],
						dest: MultiLocation::X1(Junction::Parachain(para_id)),
						effects: vec![
							Order::BuyExecution {
								fees: MultiAsset::All,
								weight: 0,
								debt,
								halt_on_error: false,
								xcm: vec![]
							},
							Order::DepositAsset {
								assets: vec![MultiAsset::All],
								dest: MultiLocation::X1(account_32.clone()),
							},
						]
					}
				]
			};

			log::info!(
				target: "ping",
				"upward&downward transfer {:?}",
				msg,
			);

			match T::XcmSender::send_xcm(MultiLocation::X1(Junction::Parent), msg) {
				Ok(()) => {
					Self::deposit_event(Event::TestMsg(some_value as u32));
					log::info!(
						target: "ping",
						"upward&downward transfer sent success!"
					);
				},
				Err(e) => {
					Self::deposit_event(Event::ErrorSendingTest());
					log::error!(
						target: "ping",
						"upward&downward transfer sent failed:{:?}",
						e,
					);
				}
			}
			Ok(())
		}
	}
}
