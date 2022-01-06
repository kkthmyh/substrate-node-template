#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::{dispatch::DispatchResult, pallet_prelude::*, traits::Randomness};
    use frame_system::pallet_prelude::*;
    use codec::{Encode, Decode};
    use frame_support::traits::Currency;
    use scale_info::TypeInfo;
    use sp_io::hashing::blake2_128;

    #[cfg(feature = "std")]
    use frame_support::serde::{Deserialize, Serialize};

    #[derive(Encode, Decode, TypeInfo)]
    pub struct Kitty(pub [u8; 16]);

    type KittyIndex = u32;

    /// Configure the pallet by specifying the parameters and types on which it depends.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Because this pallet emits events, it depends on the runtime's definition of an event.
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        type Randomness: Randomness<Self::Hash, Self::BlockNumber>;
        // 货币
        type Currency: Currency<Self::AccountId>;
    }

    #[pallet::pallet]
    #[pallet::generate_store(pub (super) trait Store)]
    pub struct Pallet<T>(_);

    // The pallet's runtime storage items.
    // https://substrate.dev/docs/en/knowledgebase/runtime/storage
    #[pallet::storage]
    #[pallet::getter(fn kitties_count)]
    pub type KittiesCount<T> = StorageValue<_, u32>;

    #[pallet::storage]
    #[pallet::getter(fn kitties)]
    pub type Kitties<T: Config> = StorageMap<_, Blake2_128Concat, KittyIndex, Option<Kitty>, ValueQuery>;
    // 所有者
    #[pallet::storage]
    #[pallet::getter(fn owner)]
    pub type Owner<T: Config> = StorageMap<_, Blake2_128Concat, KittyIndex, Option<T::AccountId>, ValueQuery>;

    // Pallets use events to inform users when important changes are made.
    // https://substrate.dev/docs/en/knowledgebase/runtime/events
    #[pallet::event]
    #[pallet::generate_deposit(pub (super) fn deposit_event)]
    pub enum Event<T: Config> {
        KittyCreate(T::AccountId, KittyIndex),
        KittyTransfer(T::AccountId, T::AccountId, KittyIndex),
    }

    // Errors inform users that something went wrong.
    #[pallet::error]
    pub enum Error<T> {
        KittiesCountOverflow,
        NotOwner,
        SameParentIndex,
        InvalidKittyIndex,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    // Dispatchable functions allows users to interact with the pallet and invoke state changes.
    // These functions materialize as "extrinsics", which are often compared to transactions.
    // Dispatchable functions must be annotated with a weight and must return a DispatchResult.
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::weight(0)]
        /// 创建kitty
        pub fn create(origin: OriginFor<T>) -> DispatchResult {
            // 验证签名 返回accountID
            let who = ensure_signed(origin)?;
            // 获取当前的kitty id
            let kitty_id = match Self::kitties_count() {
                Some(id) => {
                    ensure!(id != KittyIndex::max_value(), Error::<T>::KittiesCountOverflow);
                    id
                }
                None => 1
            };
            // 获取随机DNA
            let dna = Self::random_value(&who);
            // 插入相关数据
            Kitties::<T>::insert(kitty_id, Some(Kitty(dna)));
            Owner::<T>::insert(kitty_id, Some(who.clone()));
            KittiesCount::<T>::put(kitty_id + 1);
            // 发布创建事件
            Self::deposit_event(Event::KittyCreate(who, kitty_id));
            Ok(())
        }

        /// 转移Kitty
        #[pallet::weight(0)]
        pub fn transfer(origin: OriginFor<T>, new_owner: T::AccountId, kitty_id: KittyIndex) -> DispatchResult {
            // 验证签名 返回accountID
            let who = ensure_signed(origin)?;
            // 校验是否是原拥有者
            ensure!(Some(who.clone()) == Owner::<T>::get(kitty_id), Error::<T>::NotOwner);
            // 更新Kitty的所有者为新的拥有者
            Owner::<T>::insert(kitty_id, Some(new_owner.clone()));
            // 发布转移事件
            Self::deposit_event(Event::KittyTransfer(who, new_owner, kitty_id));
            Ok(())
        }

        /// 繁殖Kitty
        #[pallet::weight(0)]
        pub fn breed(origin: OriginFor<T>, kitty_id_1: KittyIndex, kitty_id_2: KittyIndex) -> DispatchResult {
            // 验证签名 返回accountID
            let who = ensure_signed(origin)?;
            // 校验是父母是否是同一个
            ensure!(kitty_id_1 != kitty_id_2, Error::<T>::SameParentIndex);
            // 获取Kitty1、Kitty2
            let kitty1 = Self::kitties(kitty_id_1).ok_or(Error::<T>::InvalidKittyIndex)?;
            let kitty2 = Self::kitties(kitty_id_2).ok_or(Error::<T>::InvalidKittyIndex)?;

            // 获取当前的kitty id
            let kitty_id = match Self::kitties_count() {
                Some(id) => {
                    ensure!(id != KittyIndex::max_value(), Error::<T>::KittiesCountOverflow);
                    id
                }
                None => 1
            };
            // 获取DNA
            let dna_1 = kitty1.0;
            let dna_2 = kitty2.0;
            // 混淆DNA
            let selector = Self::random_value(&who);
            let mut new_dna = [0u8; 16];
            // 位运算产生新的DNA
            for i in 0..dna_1.len() {
                new_dna[i] = (selector[i] & dna_1[i]) | (!selector[i] & dna_2[i]);
            }

            // 插入相关数据
            Kitties::<T>::insert(kitty_id, Some(Kitty(new_dna)));
            Owner::<T>::insert(kitty_id, Some(who.clone()));
            KittiesCount::<T>::put(kitty_id + 1);

            // 发布创建事件
            Self::deposit_event(Event::KittyCreate(who, kitty_id));
            Ok(())
        }

    }


    impl<T: Config> Pallet<T> {
        // 获取随机数
        fn random_value(sender: &T::AccountId) -> [u8; 16] {
            let payload = (
                T::Randomness::random_seed(),
                &sender,
                <frame_system::Pallet<T>>::extrinsic_index(),
            );
            payload.using_encoded(blake2_128)
        }
    }
}
