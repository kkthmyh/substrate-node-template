#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::{
        dispatch::DispatchResult,
        ensure,
        pallet_prelude::*,
        sp_runtime::traits::{AtLeast32BitUnsigned, Bounded},
        traits::{Currency, ExistenceRequirement, Randomness, ReservableCurrency},
    };
    use frame_system::{ensure_signed, pallet_prelude::*};
    use scale_info::TypeInfo;
    use sp_io::hashing::blake2_128;

    #[derive(Encode, Decode, TypeInfo)]
    pub struct Kitty {
        pub dna: [u8; 16],
    }

    type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    #[pallet::storage]
    #[pallet::getter(fn kitty_cnt)]
    pub(super) type KittyCnt<T: Config> = StorageValue<_, T::KittyIndex>;

    #[pallet::storage]
    #[pallet::getter(fn kitties)]
    pub type Kitties<T: Config> = StorageMap<_, Blake2_128Concat, T::KittyIndex, Option<Kitty>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn owner)]
    pub type Owner<T: Config> = StorageMap<_, Blake2_128Concat, T::KittyIndex, Option<T::AccountId>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn kitties_list_for_sales)]
    pub type ListForSale<T: Config> = StorageMap<_, Blake2_128Concat, T::KittyIndex, Option<BalanceOf<T>>, ValueQuery>;

    #[pallet::pallet]
    #[pallet::generate_store(pub (super) trait Store)]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        type Randomness: Randomness<Self::Hash, Self::BlockNumber>;
        type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;
        #[pallet::constant]
        type StakeForEachKitty: Get<BalanceOf<Self>>;
        type KittyIndex: Parameter + AtLeast32BitUnsigned + Default + Copy + Bounded;
    }

    // Errors.
    #[pallet::error]
    pub enum Error<T> {
        KittiesCountOverflow,
        NotOwner,
        SameParentIndex,
        InvalidKittyIndex,
        BuyerIsOwner,
        KittyNotForSell,
        NotEnoughBalanceForBuying,
        NotEnoughBalanceForStaking,
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub (super) fn deposit_event)]
    pub enum Event<T: Config> {
        KittyCreate(T::AccountId, T::KittyIndex),
        KittyTransfer(T::AccountId, T::AccountId, T::KittyIndex),
        KittyListed(T::AccountId, T::KittyIndex, Option<BalanceOf<T>>),
        KittySold(T::AccountId, T::AccountId, T::KittyIndex),
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        // 创建
        #[pallet::weight(0)]
        pub fn create(origin: OriginFor<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // 随机生成DNA
            let dna = Self::random_value(&who);
            // 创建kitty时质押一定数量的token
            Self::create_kitty_with_stake(&who, dna)
        }

        // 繁殖
        #[pallet::weight(0)]
        pub fn breed(origin: OriginFor<T>, kitty_id_1: T::KittyIndex, kitty_id_2: T::KittyIndex) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // parent_id 不能相同
            ensure!(kitty_id_1 != kitty_id_2, Error::<T>::SameParentIndex);
            // 获取Kitty1
            let kitty1 = Self::kitties(kitty_id_1).ok_or(Error::<T>::InvalidKittyIndex)?;
            // 获取Kitty2
            let kitty2 = Self::kitties(kitty_id_2).ok_or(Error::<T>::InvalidKittyIndex)?;

            // 获取Parents Kitty的DNA
            let dna_1 = kitty1.dna;
            let dna_2 = kitty2.dna;
            // 混淆DNA
            let selector = Self::random_value(&who);
            let mut new_dna = [0u8; 16];
            for i in 0..dna_1.len() {
                new_dna[i] = (selector[i] & dna_1[i]) | (!selector[i] & dna_2[i]);
            }
            // 创建kitty时质押一定数量的token
            Self::create_kitty_with_stake(&who, new_dna)
        }

        // 卖出
        #[pallet::weight(0)]
        pub fn sell(origin: OriginFor<T>, kitty_id: T::KittyIndex, price: Option<BalanceOf<T>>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // 校验发起方是否是该kitty的所有者
            ensure!(Some(who.clone()) == Owner::<T>::get(kitty_id), Error::<T>::NotOwner);
            // 将Kitty添加到出售列表
            ListForSale::<T>::insert(kitty_id, price);
            // 发出卖出事件
            Self::deposit_event(Event::KittyListed(who, kitty_id, price));
            Ok(())
        }

        // 转移
        #[pallet::weight(0)]
        pub fn transfer(origin: OriginFor<T>, new_owner: T::AccountId, kitty_id: T::KittyIndex) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // 检查是否是原拥有者
            ensure!(Some(who.clone()) == Owner::<T>::get(kitty_id), Error::<T>::NotOwner);
            // 更新Kitty的拥有者（双方分别释放和重新质押）
            // 获取质押金额
            let stake_amount = T::StakeForEachKitty::get();
            // 质押新的拥有者一定金额
            T::Currency::reserve(&new_owner, stake_amount).map_err(|_| Error::<T>::NotEnoughBalanceForStaking)?;
            // 解除旧拥有者的质押
            T::Currency::unreserve(&who, stake_amount);
            // 更新Kitty的所有者为新的拥有者
            Owner::<T>::insert(kitty_id, Some(new_owner.clone()));
            // 发布转移事件
            Self::deposit_event(Event::KittyTransfer(who, new_owner, kitty_id));
            Ok(())
        }

        // 购买
        #[pallet::weight(0)]
        pub fn buy(origin: OriginFor<T>, kitty_id: T::KittyIndex) -> DispatchResult {
            let buyer = ensure_signed(origin)?;
            // 获取Kitty的所有者
            let seller = Owner::<T>::get(kitty_id).unwrap();
            // 检查购买者和所有者是否是同一个人
            ensure!(Some(buyer.clone()) != Some(seller.clone()), Error::<T>::BuyerIsOwner);
            // 获取Kitty的价格，如果不存在表示Kitty不出售
            let kitty_price = ListForSale::<T>::get(kitty_id).ok_or(Error::<T>::KittyNotForSell)?;
            // 获取买家余额
            let buyer_balance = T::Currency::free_balance(&buyer);
            // 质押的金额
            let stake_amount = T::StakeForEachKitty::get();
            // 检查买家余额是否足够
            ensure!(buyer_balance > (kitty_price + stake_amount),Error::<T>::NotEnoughBalanceForBuying);
            // 质押新的拥有者一定金额
            T::Currency::reserve(&buyer, stake_amount).map_err(|_| Error::<T>::NotEnoughBalanceForStaking)?;
            // 解除旧拥有者的质押
            T::Currency::unreserve(&seller, stake_amount);
            // 买家向卖家转账
            T::Currency::transfer(&buyer, &seller, kitty_price, ExistenceRequirement::KeepAlive)?;
            // 更新Kitty的所有者为买家
            Owner::<T>::insert(kitty_id, Some(buyer.clone()));
            // 将Kitty从出售列表中移除
            ListForSale::<T>::remove(kitty_id);
            // 发出交易完成事件
            Self::deposit_event(Event::KittySold(buyer, seller, kitty_id));
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        // 随机生成DNA
        fn random_value(sender: &T::AccountId) -> [u8; 16] {
            let payload = (
                T::Randomness::random_seed(),
                &sender,
                <frame_system::Pallet<T>>::extrinsic_index(),
            );
            payload.using_encoded(blake2_128)
        }

        // 创建kitty时质押一定数量的token
        fn create_kitty_with_stake(owner: &T::AccountId, dna: [u8; 16]) -> DispatchResult {
            // 获取当前的kitty_id
            let kitty_id = match Self::kitty_cnt() {
                Some(id) => {
                    ensure!(id != T::KittyIndex::max_value(), Error::<T>::KittiesCountOverflow);
                    id
                }
                None => 1u32.into(),
            };
            // 获取质押的金额
            let stake_amount = T::StakeForEachKitty::get();
            // 质押创建者一定的金额,当余额不足时抛出NotEnoughBalanceForStaking异常
            T::Currency::reserve(&owner, stake_amount).map_err(|_| Error::<T>::NotEnoughBalanceForStaking)?;
            // 将Kitty加入Kitties集合
            Kitties::<T>::insert(kitty_id, Some(Kitty { dna }));
            // log
            log::info!("🎈😺 A kitty is born with ID ➡ {:?}.", kitty_id);
            // 为Kitty绑定所有人
            Owner::<T>::insert(kitty_id, Some(owner.clone()));
            // 更新当前的kitty_id
            KittyCnt::<T>::put(kitty_id + 1u32.into());
            // 发布创建事件
            Self::deposit_event(Event::KittyCreate(owner.clone(), kitty_id));
            Ok(())
        }
    }
}