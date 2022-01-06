#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    use frame_support::{
        sp_runtime::traits::Hash,
        traits::{Randomness, Currency, tokens::ExistenceRequirement},
        transactional,
    };
    use sp_io::hashing::blake2_128;
    use scale_info::TypeInfo;


    #[cfg(feature = "std")]
    use frame_support::serde::{Deserialize, Serialize};

    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo)]
    #[scale_info(skip_type_params(T))]
    // 定义kitty
    pub struct Kitty<T: Config> {
        pub dna: [u8; 16],
        pub price: Option<BalanceOf<T>>,
        pub gender: Gender,
        pub owner: AccountOf<T>,
    }

    // 定义性别
    #[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo)]
    #[scale_info(skip_type_params(T))]
    #[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
    pub enum Gender {
        Male,
        Female,
    }

    type AccountOf<T> = <T as frame_system::Config>::AccountId;
    type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    /// Configure the pallet by specifying the parameters and types on which it depends.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Because this pallet emits events, it depends on the runtime's definition of an event.
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        type KittyRandomness: Randomness<Self::Hash, Self::BlockNumber>;
        // 货币
        type Currency: Currency<Self::AccountId>;
        // 持有的kitty最大值
        #[pallet::constant]
        type MaxKittyOwned: Get<u32>;
    }

    #[pallet::pallet]
    #[pallet::generate_store(pub (super) trait Store)]
    pub struct Pallet<T>(_);

    // The pallet's runtime storage items.
    // https://substrate.dev/docs/en/knowledgebase/runtime/storage
    #[pallet::storage]
    #[pallet::getter(fn kitty_cnt)]
    /// Keeps track of the number of Kitties in existence.
    pub(super) type KittyCnt<T: Config> = StorageValue<_, u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn kitties)]
    pub(super) type Kitties<T: Config> = StorageMap<_, Twox64Concat, T::Hash, Kitty<T>, >;

    // 所有者
    #[pallet::storage]
    #[pallet::getter(fn kitties_owned)]
    /// Keeps track of what accounts own what Kitty.
    pub(super) type KittiesOwned<T: Config> = StorageMap<_, Twox64Concat, T::AccountId, BoundedVec<T::Hash, T::MaxKittyOwned>, ValueQuery, >;

    // Pallets use events to inform users when important changes are made.
    // https://substrate.dev/docs/en/knowledgebase/runtime/events
    #[pallet::event]
    #[pallet::generate_deposit(pub (super) fn deposit_event)]
    pub enum Event<T: Config> {
        Created(T::AccountId, T::Hash),
        PriceSet(T::AccountId, T::Hash, Option<BalanceOf<T>>),
        Transferred(T::AccountId, T::AccountId, T::Hash),
        Bought(T::AccountId, T::AccountId, T::Hash, BalanceOf<T>),
    }

    // Errors inform users that something went wrong.
    #[pallet::error]
    pub enum Error<T> {
        KittyCntOverflow,
        ExceedMaxKittyOwned,
        BuyerIsKittyOwner,
        TransferToSelf,
        KittyNotExist,
        NotKittyOwner,
        KittyNotForSale,
        KittyBidPriceTooLow,
        NotEnoughBalance,
    }

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub kitties: Vec<(T::AccountId, [u8; 16], Gender)>,
    }

    #[cfg(feature = "std")]
    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> GenesisConfig<T> {
            GenesisConfig { kitties: vec![] }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
        fn build(&self) {
            // When building a kitty from genesis config, we require the dna and gender to be supplied.
            for (acct, dna, gender) in &self.kitties {
                let _ = <Pallet<T>>::mint(acct, Some(dna.clone()), Some(gender.clone()));
            }
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    // Dispatchable functions allows users to interact with the pallet and invoke state changes.
    // These functions materialize as "extrinsics", which are often compared to transactions.
    // Dispatchable functions must be annotated with a weight and must return a DispatchResult.
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::weight(100)]
        /// 创建kitty
        pub fn create_kitty(origin: OriginFor<T>) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            let kitty_id = Self::mint(&sender, None, None)?;
            // 打印日志
            log::info!("A kitty is born with ID: {:?}.", kitty_id);
            // 发布创建事件
            Self::deposit_event(Event::Created(sender, kitty_id));
            Ok(())
        }

        #[pallet::weight(100)]
        /// 设置kitty价格
        pub fn set_price(origin: OriginFor<T>, kitty_id: T::Hash, new_price: Option<BalanceOf<T>>) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            // 校验发起方是否为kitty的所有人
            ensure!(Self::is_kitty_owner(&kitty_id, &sender)?, <Error<T>>::NotKittyOwner);
            // 获取当前kitty
            let mut kitty = Self::kitties(&kitty_id).ok_or(<Error<T>>::KittyNotExist)?;
            // 设置新价格
            kitty.price = new_price.clone();
            // 更新数据
            <Kitties<T>>::insert(&kitty_id, kitty);
            // 发布价格设置成功事件
            Self::deposit_event(Event::PriceSet(sender, kitty_id, new_price));
            Ok(())
        }

        // 转让kitty
        #[pallet::weight(100)]
        pub fn transfer(origin: OriginFor<T>, to: T::AccountId, kitty_id: T::Hash) -> DispatchResult {
            let from = ensure_signed(origin)?;
            // 校验发起方是否为kitty的所有人
            ensure!(Self::is_kitty_owner(&kitty_id, &from)?, <Error<T>>::NotKittyOwner);
            // 校验发起方接收方不是同一个人
            ensure!(from != to, <Error<T>>::TransferToSelf);
            // 校验接收方持有的kitty数量是否超过上限
            let to_owned = <KittiesOwned<T>>::get(&to);
            ensure!((to_owned.len() as u32) < T::MaxKittyOwned::get(), <Error<T>>::ExceedMaxKittyOwned);
            Self::transfer_kitty_to(&kitty_id, &to)?;
            Self::deposit_event(Event::Transferred(from, to, kitty_id));
            Ok(())
        }

        #[transactional]
        #[pallet::weight(100)]
        /// 购买kitty
        pub fn buy_kitty(origin: OriginFor<T>, kitty_id: T::Hash, bid_price: BalanceOf<T>) -> DispatchResult {
            let buyer = ensure_signed(origin)?;
            // 校验购买方不是当前kitty的持有者
            let kitty = Self::kitties(&kitty_id).ok_or(<Error<T>>::KittyNotExist)?;
            ensure!(kitty.owner != buyer, <Error<T>>::BuyerIsKittyOwner);
            // 校验kitty是否是可以出售的且价格小于等于bid_price
            if let Some(ask_price) = kitty.price {
                ensure!(ask_price <= bid_price, <Error<T>>::KittyBidPriceTooLow);
            } else {
                Err(<Error<T>>::KittyNotForSale)?;
            }
            // 校验购买者资金是否足够
            ensure!(T::Currency::free_balance(&buyer) >= bid_price, <Error<T>>::NotEnoughBalance);
            // 校验购买者持有数量是否超过上限
            let to_owned = <KittiesOwned<T>>::get(&buyer);
            ensure!((to_owned.len() as u32) < T::MaxKittyOwned::get(), <Error<T>>::ExceedMaxKittyOwned);
            let seller = kitty.owner.clone();
            // 交换余额
            T::Currency::transfer(&buyer, &seller, bid_price, ExistenceRequirement::KeepAlive)?;
            // 转移kitty
            Self::transfer_kitty_to(&kitty_id, &buyer)?;
            // 发布购买事件
            Self::deposit_event(Event::Bought(buyer, seller, kitty_id, bid_price));
            Ok(())
        }

        #[pallet::weight(100)]
        /// 繁殖kitty
        pub fn breed_kitty(origin: OriginFor<T>, kid1: T::Hash, kid2: T::Hash) -> DispatchResult {
            let sender = ensure_signed(origin)?;
            // 校验两只kitty都是该持有人的
            ensure!(Self::is_kitty_owner(&kid1, &sender)?, <Error<T>>::NotKittyOwner);
            ensure!(Self::is_kitty_owner(&kid2, &sender)?, <Error<T>>::NotKittyOwner);
            let new_dna = Self::breed_dna(&kid1, &kid2)?;
            Self::mint(&sender, Some(new_dna), None)?;
            Ok(())
        }
    }


    impl<T: Config> Pallet<T> {
        // 获取性别
        fn gen_gender() -> Gender {
            let random = T::KittyRandomness::random(&b"gender"[..]).0;
            match random.as_ref()[0] % 2 {
                0 => Gender::Male,
                _ => Gender::Female,
            }
        }
        // 创建kitty时生成DNA
        fn gen_dna() -> [u8; 16] {
            let payload = (
                T::KittyRandomness::random(&b"dna"[..]).0,
                <frame_system::Pallet<T>>::block_number(),
            );
            payload.using_encoded(blake2_128)
        }
        // 繁殖时生成DNA
        pub fn breed_dna(kid1: &T::Hash, kid2: &T::Hash) -> Result<[u8; 16], Error<T>> {
            let dna1 = Self::kitties(kid1).ok_or(<Error<T>>::KittyNotExist)?.dna;
            let dna2 = Self::kitties(kid2).ok_or(<Error<T>>::KittyNotExist)?.dna;
            let mut new_dna = Self::gen_dna();
            for i in 0..new_dna.len() {
                new_dna[i] = (new_dna[i] & dna1[i]) | (!new_dna[i] & dna2[i]);
            }
            Ok(new_dna)
        }

        // 抽取公共公共方法进行链上存储等操作
        pub fn mint(owner: &T::AccountId, dna: Option<[u8; 16]>, gender: Option<Gender>) -> Result<T::Hash, Error<T>> {
            let kitty = Kitty::<T> {
                dna: dna.unwrap_or_else(Self::gen_dna),
                price: None,
                gender: gender.unwrap_or_else(Self::gen_gender),
                owner: owner.clone(),
            };

            let kitty_id = T::Hashing::hash_of(&kitty);

            // Performs this operation first as it may fail
            let new_cnt = Self::kitty_cnt().checked_add(1).ok_or(<Error<T>>::KittyCntOverflow)?;

            // 存储信息
            <KittiesOwned<T>>::try_mutate(&owner, |kitty_vec| {
                kitty_vec.try_push(kitty_id)
            }).map_err(|_| <Error<T>>::ExceedMaxKittyOwned)?;

            <Kitties<T>>::insert(kitty_id, kitty);
            <KittyCnt<T>>::put(new_cnt);
            Ok(kitty_id)
        }
        // 判断是否是kitty的所有者
        pub fn is_kitty_owner(kitty_id: &T::Hash, acct: &T::AccountId) -> Result<bool, Error<T>> {
            match Self::kitties(kitty_id) {
                Some(kitty) => Ok(kitty.owner == *acct),
                None => Err(<Error<T>>::KittyNotExist)
            }
        }
        // 转让kitty具体实现
        #[transactional]
        pub fn transfer_kitty_to(kitty_id: &T::Hash, to: &T::AccountId) -> Result<(), Error<T>> {
            let mut kitty = Self::kitties(&kitty_id).ok_or(<Error<T>>::KittyNotExist)?;
            let prev_owner = kitty.owner.clone();

            // 将kitty从原所有者的列表移除
            <KittiesOwned<T>>::try_mutate(&prev_owner, |owned| {
                if let Some(ind) = owned.iter().position(|&id| id == *kitty_id) {
                    owned.swap_remove(ind);
                    return Ok(());
                }
                Err(())
            }).map_err(|_| <Error<T>>::KittyNotExist)?;

            // 修改kitty所有者
            kitty.owner = to.clone();
            // 重新设置价格
            kitty.price = None;

            <Kitties<T>>::insert(kitty_id, kitty);
            <KittiesOwned<T>>::try_mutate(to, |vec| {
                vec.try_push(*kitty_id)
            }).map_err(|_| <Error<T>>::ExceedMaxKittyOwned)?;

            Ok(())
        }
    }
}
