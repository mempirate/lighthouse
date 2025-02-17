use crate::beacon_block_body::{
    BeaconBlockBodyAltair, BeaconBlockBodyBase, BeaconBlockBodyDeneb, BeaconBlockBodyMerge,
    BeaconBlockBodyRef, BeaconBlockBodyRefMut,
};
use crate::test_utils::TestRandom;
use crate::*;
use bls::Signature;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use ssz::{Decode, DecodeError};
use ssz_derive::{Decode, Encode};
use std::marker::PhantomData;
use superstruct::superstruct;
use test_random_derive::TestRandom;
use tree_hash::TreeHash;
use tree_hash_derive::TreeHash;

/// A block of the `BeaconChain`.
#[superstruct(
    variants(Base, Altair, Merge, Capella, Deneb),
    variant_attributes(
        derive(
            Debug,
            Clone,
            Serialize,
            Deserialize,
            Encode,
            Decode,
            TreeHash,
            TestRandom,
            Derivative,
            arbitrary::Arbitrary
        ),
        derivative(PartialEq, Hash(bound = "T: EthSpec, Payload: AbstractExecPayload<T>")),
        serde(
            bound = "T: EthSpec, Payload: AbstractExecPayload<T>",
            deny_unknown_fields
        ),
        arbitrary(bound = "T: EthSpec, Payload: AbstractExecPayload<T>"),
    ),
    ref_attributes(
        derive(Debug, PartialEq, TreeHash),
        tree_hash(enum_behaviour = "transparent")
    ),
    map_ref_into(BeaconBlockBodyRef, BeaconBlock),
    map_ref_mut_into(BeaconBlockBodyRefMut)
)]
#[derive(
    Debug, Clone, Serialize, Deserialize, Encode, TreeHash, Derivative, arbitrary::Arbitrary,
)]
#[derivative(PartialEq, Hash(bound = "T: EthSpec"))]
#[serde(untagged)]
#[serde(bound = "T: EthSpec, Payload: AbstractExecPayload<T>")]
#[arbitrary(bound = "T: EthSpec, Payload: AbstractExecPayload<T>")]
#[tree_hash(enum_behaviour = "transparent")]
#[ssz(enum_behaviour = "transparent")]
pub struct BeaconBlock<T: EthSpec, Payload: AbstractExecPayload<T> = FullPayload<T>> {
    #[superstruct(getter(copy))]
    pub slot: Slot,
    #[superstruct(getter(copy))]
    #[serde(with = "serde_utils::quoted_u64")]
    pub proposer_index: u64,
    #[superstruct(getter(copy))]
    pub parent_root: Hash256,
    #[superstruct(getter(copy))]
    pub state_root: Hash256,
    #[superstruct(only(Base), partial_getter(rename = "body_base"))]
    pub body: BeaconBlockBodyBase<T, Payload>,
    #[superstruct(only(Altair), partial_getter(rename = "body_altair"))]
    pub body: BeaconBlockBodyAltair<T, Payload>,
    #[superstruct(only(Merge), partial_getter(rename = "body_merge"))]
    pub body: BeaconBlockBodyMerge<T, Payload>,
    #[superstruct(only(Capella), partial_getter(rename = "body_capella"))]
    pub body: BeaconBlockBodyCapella<T, Payload>,
    #[superstruct(only(Deneb), partial_getter(rename = "body_deneb"))]
    pub body: BeaconBlockBodyDeneb<T, Payload>,
}

pub type BlindedBeaconBlock<E> = BeaconBlock<E, BlindedPayload<E>>;

impl<T: EthSpec, Payload: AbstractExecPayload<T>> SignedRoot for BeaconBlock<T, Payload> {}
impl<'a, T: EthSpec, Payload: AbstractExecPayload<T>> SignedRoot
    for BeaconBlockRef<'a, T, Payload>
{
}

/// Empty block trait for each block variant to implement.
pub trait EmptyBlock {
    /// Returns an empty block to be used during genesis.
    fn empty(spec: &ChainSpec) -> Self;
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> BeaconBlock<T, Payload> {
    /// Returns an empty block to be used during genesis.
    pub fn empty(spec: &ChainSpec) -> Self {
        map_fork_name!(
            spec.fork_name_at_epoch(T::genesis_epoch()),
            Self,
            EmptyBlock::empty(spec)
        )
    }

    /// Custom SSZ decoder that takes a `ChainSpec` as context.
    pub fn from_ssz_bytes(bytes: &[u8], spec: &ChainSpec) -> Result<Self, ssz::DecodeError> {
        let slot_len = <Slot as Decode>::ssz_fixed_len();
        let slot_bytes = bytes
            .get(0..slot_len)
            .ok_or(DecodeError::InvalidByteLength {
                len: bytes.len(),
                expected: slot_len,
            })?;

        let slot = Slot::from_ssz_bytes(slot_bytes)?;
        let fork_at_slot = spec.fork_name_at_slot::<T>(slot);
        Self::from_ssz_bytes_for_fork(bytes, fork_at_slot)
    }

    /// Custom SSZ decoder that takes a `ForkName` as context.
    pub fn from_ssz_bytes_for_fork(
        bytes: &[u8],
        fork_name: ForkName,
    ) -> Result<Self, ssz::DecodeError> {
        Ok(map_fork_name!(fork_name, Self, <_>::from_ssz_bytes(bytes)?))
    }

    /// Try decoding each beacon block variant in sequence.
    ///
    /// This is *not* recommended unless you really have no idea what variant the block should be.
    /// Usually it's better to prefer `from_ssz_bytes` which will decode the correct variant based
    /// on the fork slot.
    pub fn any_from_ssz_bytes(bytes: &[u8]) -> Result<Self, ssz::DecodeError> {
        BeaconBlockDeneb::from_ssz_bytes(bytes)
            .map(BeaconBlock::Deneb)
            .or_else(|_| BeaconBlockCapella::from_ssz_bytes(bytes).map(BeaconBlock::Capella))
            .or_else(|_| BeaconBlockMerge::from_ssz_bytes(bytes).map(BeaconBlock::Merge))
            .or_else(|_| BeaconBlockAltair::from_ssz_bytes(bytes).map(BeaconBlock::Altair))
            .or_else(|_| BeaconBlockBase::from_ssz_bytes(bytes).map(BeaconBlock::Base))
    }

    /// Convenience accessor for the `body` as a `BeaconBlockBodyRef`.
    pub fn body(&self) -> BeaconBlockBodyRef<'_, T, Payload> {
        self.to_ref().body()
    }

    /// Convenience accessor for the `body` as a `BeaconBlockBodyRefMut`.
    pub fn body_mut(&mut self) -> BeaconBlockBodyRefMut<'_, T, Payload> {
        self.to_mut().body_mut()
    }

    /// Returns the epoch corresponding to `self.slot()`.
    pub fn epoch(&self) -> Epoch {
        self.slot().epoch(T::slots_per_epoch())
    }

    /// Returns the `tree_hash_root` of the block.
    pub fn canonical_root(&self) -> Hash256 {
        self.tree_hash_root()
    }

    /// Returns a full `BeaconBlockHeader` of this block.
    ///
    /// Note: This method is used instead of an `Into` impl to avoid a `Clone` of an entire block
    /// when you want to have the block _and_ the header.
    ///
    /// Note: performs a full tree-hash of `self.body`.
    pub fn block_header(&self) -> BeaconBlockHeader {
        self.to_ref().block_header()
    }

    /// Returns a "temporary" header, where the `state_root` is `Hash256::zero()`.
    pub fn temporary_block_header(&self) -> BeaconBlockHeader {
        self.to_ref().temporary_block_header()
    }

    /// Return the tree hash root of the block's body.
    pub fn body_root(&self) -> Hash256 {
        self.to_ref().body_root()
    }

    /// Signs `self`, producing a `SignedBeaconBlock`.
    pub fn sign(
        self,
        secret_key: &SecretKey,
        fork: &Fork,
        genesis_validators_root: Hash256,
        spec: &ChainSpec,
    ) -> SignedBeaconBlock<T, Payload> {
        let domain = spec.get_domain(
            self.epoch(),
            Domain::BeaconProposer,
            fork,
            genesis_validators_root,
        );
        let message = self.signing_root(domain);
        let signature = secret_key.sign(message);
        SignedBeaconBlock::from_block(self, signature)
    }
}

impl<'a, T: EthSpec, Payload: AbstractExecPayload<T>> BeaconBlockRef<'a, T, Payload> {
    /// Returns the name of the fork pertaining to `self`.
    ///
    /// Will return an `Err` if `self` has been instantiated to a variant conflicting with the fork
    /// dictated by `self.slot()`.
    pub fn fork_name(&self, spec: &ChainSpec) -> Result<ForkName, InconsistentFork> {
        let fork_at_slot = spec.fork_name_at_slot::<T>(self.slot());
        let object_fork = self.fork_name_unchecked();

        if fork_at_slot == object_fork {
            Ok(object_fork)
        } else {
            Err(InconsistentFork {
                fork_at_slot,
                object_fork,
            })
        }
    }

    /// Returns the name of the fork pertaining to `self`.
    ///
    /// Does not check that the fork is consistent with the slot.
    pub fn fork_name_unchecked(&self) -> ForkName {
        match self {
            BeaconBlockRef::Base { .. } => ForkName::Base,
            BeaconBlockRef::Altair { .. } => ForkName::Altair,
            BeaconBlockRef::Merge { .. } => ForkName::Merge,
            BeaconBlockRef::Capella { .. } => ForkName::Capella,
            BeaconBlockRef::Deneb { .. } => ForkName::Deneb,
        }
    }

    /// Convenience accessor for the `body` as a `BeaconBlockBodyRef`.
    pub fn body(&self) -> BeaconBlockBodyRef<'a, T, Payload> {
        map_beacon_block_ref_into_beacon_block_body_ref!(&'a _, *self, |block, cons| cons(
            &block.body
        ))
    }

    /// Return the tree hash root of the block's body.
    pub fn body_root(&self) -> Hash256 {
        map_beacon_block_ref!(&'a _, *self, |block, cons| {
            let _: Self = cons(block);
            block.body.tree_hash_root()
        })
    }

    /// Returns the epoch corresponding to `self.slot()`.
    pub fn epoch(&self) -> Epoch {
        self.slot().epoch(T::slots_per_epoch())
    }

    /// Returns a full `BeaconBlockHeader` of this block.
    pub fn block_header(&self) -> BeaconBlockHeader {
        BeaconBlockHeader {
            slot: self.slot(),
            proposer_index: self.proposer_index(),
            parent_root: self.parent_root(),
            state_root: self.state_root(),
            body_root: self.body_root(),
        }
    }

    /// Returns a "temporary" header, where the `state_root` is `Hash256::zero()`.
    pub fn temporary_block_header(self) -> BeaconBlockHeader {
        BeaconBlockHeader {
            state_root: Hash256::zero(),
            ..self.block_header()
        }
    }

    /// Extracts a reference to an execution payload from a block, returning an error if the block
    /// is pre-merge.
    pub fn execution_payload(&self) -> Result<Payload::Ref<'a>, Error> {
        self.body().execution_payload()
    }
}

impl<'a, T: EthSpec, Payload: AbstractExecPayload<T>> BeaconBlockRefMut<'a, T, Payload> {
    /// Convert a mutable reference to a beacon block to a mutable ref to its body.
    pub fn body_mut(self) -> BeaconBlockBodyRefMut<'a, T, Payload> {
        map_beacon_block_ref_mut_into_beacon_block_body_ref_mut!(&'a _, self, |block, cons| cons(
            &mut block.body
        ))
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> EmptyBlock for BeaconBlockBase<T, Payload> {
    fn empty(spec: &ChainSpec) -> Self {
        BeaconBlockBase {
            slot: spec.genesis_slot,
            proposer_index: 0,
            parent_root: Hash256::zero(),
            state_root: Hash256::zero(),
            body: BeaconBlockBodyBase {
                randao_reveal: Signature::empty(),
                eth1_data: Eth1Data {
                    deposit_root: Hash256::zero(),
                    block_hash: Hash256::zero(),
                    deposit_count: 0,
                },
                graffiti: Graffiti::default(),
                proposer_slashings: VariableList::empty(),
                attester_slashings: VariableList::empty(),
                attestations: VariableList::empty(),
                deposits: VariableList::empty(),
                voluntary_exits: VariableList::empty(),
                _phantom: PhantomData,
            },
        }
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> BeaconBlockBase<T, Payload> {
    /// Return a block where the block has maximum size.
    pub fn full(spec: &ChainSpec) -> Self {
        let header = BeaconBlockHeader {
            slot: Slot::new(1),
            proposer_index: 0,
            parent_root: Hash256::zero(),
            state_root: Hash256::zero(),
            body_root: Hash256::zero(),
        };

        let signed_header = SignedBeaconBlockHeader {
            message: header,
            signature: Signature::empty(),
        };
        let indexed_attestation: IndexedAttestation<T> = IndexedAttestation {
            attesting_indices: VariableList::new(vec![
                0_u64;
                T::MaxValidatorsPerCommittee::to_usize()
            ])
            .unwrap(),
            data: AttestationData::default(),
            signature: AggregateSignature::empty(),
        };

        let deposit_data = DepositData {
            pubkey: PublicKeyBytes::empty(),
            withdrawal_credentials: Hash256::zero(),
            amount: 0,
            signature: SignatureBytes::empty(),
        };
        let proposer_slashing = ProposerSlashing {
            signed_header_1: signed_header.clone(),
            signed_header_2: signed_header,
        };

        let attester_slashing = AttesterSlashing {
            attestation_1: indexed_attestation.clone(),
            attestation_2: indexed_attestation,
        };

        let attestation: Attestation<T> = Attestation {
            aggregation_bits: BitList::with_capacity(T::MaxValidatorsPerCommittee::to_usize())
                .unwrap(),
            data: AttestationData::default(),
            signature: AggregateSignature::empty(),
        };

        let deposit = Deposit {
            proof: FixedVector::from_elem(Hash256::zero()),
            data: deposit_data,
        };

        let voluntary_exit = VoluntaryExit {
            epoch: Epoch::new(1),
            validator_index: 1,
        };

        let signed_voluntary_exit = SignedVoluntaryExit {
            message: voluntary_exit,
            signature: Signature::empty(),
        };

        let mut block = BeaconBlockBase::<T, Payload>::empty(spec);
        for _ in 0..T::MaxProposerSlashings::to_usize() {
            block
                .body
                .proposer_slashings
                .push(proposer_slashing.clone())
                .unwrap();
        }
        for _ in 0..T::MaxDeposits::to_usize() {
            block.body.deposits.push(deposit.clone()).unwrap();
        }
        for _ in 0..T::MaxVoluntaryExits::to_usize() {
            block
                .body
                .voluntary_exits
                .push(signed_voluntary_exit.clone())
                .unwrap();
        }
        for _ in 0..T::MaxAttesterSlashings::to_usize() {
            block
                .body
                .attester_slashings
                .push(attester_slashing.clone())
                .unwrap();
        }

        for _ in 0..T::MaxAttestations::to_usize() {
            block.body.attestations.push(attestation.clone()).unwrap();
        }
        block
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> EmptyBlock for BeaconBlockAltair<T, Payload> {
    /// Returns an empty Altair block to be used during genesis.
    fn empty(spec: &ChainSpec) -> Self {
        BeaconBlockAltair {
            slot: spec.genesis_slot,
            proposer_index: 0,
            parent_root: Hash256::zero(),
            state_root: Hash256::zero(),
            body: BeaconBlockBodyAltair {
                randao_reveal: Signature::empty(),
                eth1_data: Eth1Data {
                    deposit_root: Hash256::zero(),
                    block_hash: Hash256::zero(),
                    deposit_count: 0,
                },
                graffiti: Graffiti::default(),
                proposer_slashings: VariableList::empty(),
                attester_slashings: VariableList::empty(),
                attestations: VariableList::empty(),
                deposits: VariableList::empty(),
                voluntary_exits: VariableList::empty(),
                sync_aggregate: SyncAggregate::empty(),
                _phantom: PhantomData,
            },
        }
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> BeaconBlockAltair<T, Payload> {
    /// Return an Altair block where the block has maximum size.
    pub fn full(spec: &ChainSpec) -> Self {
        let base_block: BeaconBlockBase<_, Payload> = BeaconBlockBase::full(spec);
        let sync_aggregate = SyncAggregate {
            sync_committee_signature: AggregateSignature::empty(),
            sync_committee_bits: BitVector::default(),
        };
        BeaconBlockAltair {
            slot: spec.genesis_slot,
            proposer_index: 0,
            parent_root: Hash256::zero(),
            state_root: Hash256::zero(),
            body: BeaconBlockBodyAltair {
                proposer_slashings: base_block.body.proposer_slashings,
                attester_slashings: base_block.body.attester_slashings,
                attestations: base_block.body.attestations,
                deposits: base_block.body.deposits,
                voluntary_exits: base_block.body.voluntary_exits,
                sync_aggregate,
                randao_reveal: Signature::empty(),
                eth1_data: Eth1Data {
                    deposit_root: Hash256::zero(),
                    block_hash: Hash256::zero(),
                    deposit_count: 0,
                },
                graffiti: Graffiti::default(),
                _phantom: PhantomData,
            },
        }
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> EmptyBlock for BeaconBlockMerge<T, Payload> {
    /// Returns an empty Merge block to be used during genesis.
    fn empty(spec: &ChainSpec) -> Self {
        BeaconBlockMerge {
            slot: spec.genesis_slot,
            proposer_index: 0,
            parent_root: Hash256::zero(),
            state_root: Hash256::zero(),
            body: BeaconBlockBodyMerge {
                randao_reveal: Signature::empty(),
                eth1_data: Eth1Data {
                    deposit_root: Hash256::zero(),
                    block_hash: Hash256::zero(),
                    deposit_count: 0,
                },
                graffiti: Graffiti::default(),
                proposer_slashings: VariableList::empty(),
                attester_slashings: VariableList::empty(),
                attestations: VariableList::empty(),
                deposits: VariableList::empty(),
                voluntary_exits: VariableList::empty(),
                sync_aggregate: SyncAggregate::empty(),
                execution_payload: Payload::Merge::default(),
            },
        }
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> BeaconBlockCapella<T, Payload> {
    /// Return a Capella block where the block has maximum size.
    pub fn full(spec: &ChainSpec) -> Self {
        let base_block: BeaconBlockBase<_, Payload> = BeaconBlockBase::full(spec);
        let bls_to_execution_changes = vec![
            SignedBlsToExecutionChange {
                message: BlsToExecutionChange {
                    validator_index: 0,
                    from_bls_pubkey: PublicKeyBytes::empty(),
                    to_execution_address: Address::zero(),
                },
                signature: Signature::empty()
            };
            T::max_bls_to_execution_changes()
        ]
        .into();
        let sync_aggregate = SyncAggregate {
            sync_committee_signature: AggregateSignature::empty(),
            sync_committee_bits: BitVector::default(),
        };
        BeaconBlockCapella {
            slot: spec.genesis_slot,
            proposer_index: 0,
            parent_root: Hash256::zero(),
            state_root: Hash256::zero(),
            body: BeaconBlockBodyCapella {
                proposer_slashings: base_block.body.proposer_slashings,
                attester_slashings: base_block.body.attester_slashings,
                attestations: base_block.body.attestations,
                deposits: base_block.body.deposits,
                voluntary_exits: base_block.body.voluntary_exits,
                bls_to_execution_changes,
                sync_aggregate,
                randao_reveal: Signature::empty(),
                eth1_data: Eth1Data {
                    deposit_root: Hash256::zero(),
                    block_hash: Hash256::zero(),
                    deposit_count: 0,
                },
                graffiti: Graffiti::default(),
                execution_payload: Payload::Capella::default(),
            },
        }
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> EmptyBlock for BeaconBlockCapella<T, Payload> {
    /// Returns an empty Capella block to be used during genesis.
    fn empty(spec: &ChainSpec) -> Self {
        BeaconBlockCapella {
            slot: spec.genesis_slot,
            proposer_index: 0,
            parent_root: Hash256::zero(),
            state_root: Hash256::zero(),
            body: BeaconBlockBodyCapella {
                randao_reveal: Signature::empty(),
                eth1_data: Eth1Data {
                    deposit_root: Hash256::zero(),
                    block_hash: Hash256::zero(),
                    deposit_count: 0,
                },
                graffiti: Graffiti::default(),
                proposer_slashings: VariableList::empty(),
                attester_slashings: VariableList::empty(),
                attestations: VariableList::empty(),
                deposits: VariableList::empty(),
                voluntary_exits: VariableList::empty(),
                sync_aggregate: SyncAggregate::empty(),
                execution_payload: Payload::Capella::default(),
                bls_to_execution_changes: VariableList::empty(),
            },
        }
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> EmptyBlock for BeaconBlockDeneb<T, Payload> {
    /// Returns an empty Deneb block to be used during genesis.
    fn empty(spec: &ChainSpec) -> Self {
        BeaconBlockDeneb {
            slot: spec.genesis_slot,
            proposer_index: 0,
            parent_root: Hash256::zero(),
            state_root: Hash256::zero(),
            body: BeaconBlockBodyDeneb {
                randao_reveal: Signature::empty(),
                eth1_data: Eth1Data {
                    deposit_root: Hash256::zero(),
                    block_hash: Hash256::zero(),
                    deposit_count: 0,
                },
                graffiti: Graffiti::default(),
                proposer_slashings: VariableList::empty(),
                attester_slashings: VariableList::empty(),
                attestations: VariableList::empty(),
                deposits: VariableList::empty(),
                voluntary_exits: VariableList::empty(),
                sync_aggregate: SyncAggregate::empty(),
                execution_payload: Payload::Deneb::default(),
                bls_to_execution_changes: VariableList::empty(),
                blob_kzg_commitments: VariableList::empty(),
            },
        }
    }
}

// We can convert pre-Bellatrix blocks without payloads into blocks "with" payloads.
impl<E: EthSpec> From<BeaconBlockBase<E, BlindedPayload<E>>>
    for BeaconBlockBase<E, FullPayload<E>>
{
    fn from(block: BeaconBlockBase<E, BlindedPayload<E>>) -> Self {
        let BeaconBlockBase {
            slot,
            proposer_index,
            parent_root,
            state_root,
            body,
        } = block;

        BeaconBlockBase {
            slot,
            proposer_index,
            parent_root,
            state_root,
            body: body.into(),
        }
    }
}

impl<E: EthSpec> From<BeaconBlockAltair<E, BlindedPayload<E>>>
    for BeaconBlockAltair<E, FullPayload<E>>
{
    fn from(block: BeaconBlockAltair<E, BlindedPayload<E>>) -> Self {
        let BeaconBlockAltair {
            slot,
            proposer_index,
            parent_root,
            state_root,
            body,
        } = block;

        BeaconBlockAltair {
            slot,
            proposer_index,
            parent_root,
            state_root,
            body: body.into(),
        }
    }
}

// We can convert blocks with payloads to blocks without payloads, and an optional payload.
macro_rules! impl_from {
    ($ty_name:ident, <$($from_params:ty),*>, <$($to_params:ty),*>, $body_expr:expr) => {
        impl<E: EthSpec> From<$ty_name<$($from_params),*>>
            for ($ty_name<$($to_params),*>, Option<ExecutionPayload<E>>)
        {
            #[allow(clippy::redundant_closure_call)]
            fn from(block: $ty_name<$($from_params),*>) -> Self {
                let $ty_name {
                    slot,
                    proposer_index,
                    parent_root,
                    state_root,
                    body,
                } = block;

                let (body, payload) = ($body_expr)(body);

                ($ty_name {
                    slot,
                    proposer_index,
                    parent_root,
                    state_root,
                    body,
                }, payload.map(Into::into))
            }
        }
    }
}

impl_from!(BeaconBlockBase, <E, FullPayload<E>>, <E, BlindedPayload<E>>, |body: BeaconBlockBodyBase<_, _>| body.into());
impl_from!(BeaconBlockAltair, <E, FullPayload<E>>, <E, BlindedPayload<E>>, |body: BeaconBlockBodyAltair<_, _>| body.into());
impl_from!(BeaconBlockMerge, <E, FullPayload<E>>, <E, BlindedPayload<E>>, |body: BeaconBlockBodyMerge<_, _>| body.into());
impl_from!(BeaconBlockCapella, <E, FullPayload<E>>, <E, BlindedPayload<E>>, |body: BeaconBlockBodyCapella<_, _>| body.into());
impl_from!(BeaconBlockDeneb, <E, FullPayload<E>>, <E, BlindedPayload<E>>, |body: BeaconBlockBodyDeneb<_, _>| body.into());

// We can clone blocks with payloads to blocks without payloads, without cloning the payload.
macro_rules! impl_clone_as_blinded {
    ($ty_name:ident, <$($from_params:ty),*>, <$($to_params:ty),*>) => {
        impl<E: EthSpec> $ty_name<$($from_params),*>
        {
            pub fn clone_as_blinded(&self) -> $ty_name<$($to_params),*> {
                let $ty_name {
                    slot,
                    proposer_index,
                    parent_root,
                    state_root,
                    body,
                } = self;

                $ty_name {
                    slot: *slot,
                    proposer_index: *proposer_index,
                    parent_root: *parent_root,
                    state_root: *state_root,
                    body: body.clone_as_blinded(),
                }
            }
        }
    }
}

impl_clone_as_blinded!(BeaconBlockBase, <E, FullPayload<E>>, <E, BlindedPayload<E>>);
impl_clone_as_blinded!(BeaconBlockAltair, <E, FullPayload<E>>, <E, BlindedPayload<E>>);
impl_clone_as_blinded!(BeaconBlockMerge, <E, FullPayload<E>>, <E, BlindedPayload<E>>);
impl_clone_as_blinded!(BeaconBlockCapella, <E, FullPayload<E>>, <E, BlindedPayload<E>>);
impl_clone_as_blinded!(BeaconBlockDeneb, <E, FullPayload<E>>, <E, BlindedPayload<E>>);

// A reference to a full beacon block can be cloned into a blinded beacon block, without cloning the
// execution payload.
impl<'a, E: EthSpec> From<BeaconBlockRef<'a, E, FullPayload<E>>>
    for BeaconBlock<E, BlindedPayload<E>>
{
    fn from(
        full_block: BeaconBlockRef<'a, E, FullPayload<E>>,
    ) -> BeaconBlock<E, BlindedPayload<E>> {
        map_beacon_block_ref_into_beacon_block!(&'a _, full_block, |inner, cons| {
            cons(inner.clone_as_blinded())
        })
    }
}

impl<E: EthSpec> From<BeaconBlock<E, FullPayload<E>>>
    for (
        BeaconBlock<E, BlindedPayload<E>>,
        Option<ExecutionPayload<E>>,
    )
{
    fn from(block: BeaconBlock<E, FullPayload<E>>) -> Self {
        map_beacon_block!(block, |inner, cons| {
            let (block, payload) = inner.into();
            (cons(block), payload)
        })
    }
}

impl<T: EthSpec, Payload: AbstractExecPayload<T>> ForkVersionDeserialize
    for BeaconBlock<T, Payload>
{
    fn deserialize_by_fork<'de, D: serde::Deserializer<'de>>(
        value: serde_json::value::Value,
        fork_name: ForkName,
    ) -> Result<Self, D::Error> {
        Ok(map_fork_name!(
            fork_name,
            Self,
            serde_json::from_value(value).map_err(|e| serde::de::Error::custom(format!(
                "BeaconBlock failed to deserialize: {:?}",
                e
            )))?
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{test_ssz_tree_hash_pair_with, SeedableRng, TestRandom, XorShiftRng};
    use crate::{ForkName, MainnetEthSpec};
    use ssz::Encode;

    type BeaconBlock = super::BeaconBlock<MainnetEthSpec>;
    type BeaconBlockBase = super::BeaconBlockBase<MainnetEthSpec>;
    type BeaconBlockAltair = super::BeaconBlockAltair<MainnetEthSpec>;

    #[test]
    fn roundtrip_base_block() {
        let rng = &mut XorShiftRng::from_seed([42; 16]);
        let spec = &ForkName::Base.make_genesis_spec(MainnetEthSpec::default_spec());

        let inner_block = BeaconBlockBase {
            slot: Slot::random_for_test(rng),
            proposer_index: u64::random_for_test(rng),
            parent_root: Hash256::random_for_test(rng),
            state_root: Hash256::random_for_test(rng),
            body: BeaconBlockBodyBase::random_for_test(rng),
        };
        let block = BeaconBlock::Base(inner_block.clone());

        test_ssz_tree_hash_pair_with(&block, &inner_block, |bytes| {
            BeaconBlock::from_ssz_bytes(bytes, spec)
        });
    }

    #[test]
    fn roundtrip_altair_block() {
        let rng = &mut XorShiftRng::from_seed([42; 16]);
        let spec = &ForkName::Altair.make_genesis_spec(MainnetEthSpec::default_spec());

        let inner_block = BeaconBlockAltair {
            slot: Slot::random_for_test(rng),
            proposer_index: u64::random_for_test(rng),
            parent_root: Hash256::random_for_test(rng),
            state_root: Hash256::random_for_test(rng),
            body: BeaconBlockBodyAltair::random_for_test(rng),
        };
        let block = BeaconBlock::Altair(inner_block.clone());

        test_ssz_tree_hash_pair_with(&block, &inner_block, |bytes| {
            BeaconBlock::from_ssz_bytes(bytes, spec)
        });
    }

    #[test]
    fn roundtrip_capella_block() {
        let rng = &mut XorShiftRng::from_seed([42; 16]);
        let spec = &ForkName::Capella.make_genesis_spec(MainnetEthSpec::default_spec());

        let inner_block = BeaconBlockCapella {
            slot: Slot::random_for_test(rng),
            proposer_index: u64::random_for_test(rng),
            parent_root: Hash256::random_for_test(rng),
            state_root: Hash256::random_for_test(rng),
            body: BeaconBlockBodyCapella::random_for_test(rng),
        };
        let block = BeaconBlock::Capella(inner_block.clone());

        test_ssz_tree_hash_pair_with(&block, &inner_block, |bytes| {
            BeaconBlock::from_ssz_bytes(bytes, spec)
        });
    }

    #[test]
    fn roundtrip_4844_block() {
        let rng = &mut XorShiftRng::from_seed([42; 16]);
        let spec = &ForkName::Deneb.make_genesis_spec(MainnetEthSpec::default_spec());

        let inner_block = BeaconBlockDeneb {
            slot: Slot::random_for_test(rng),
            proposer_index: u64::random_for_test(rng),
            parent_root: Hash256::random_for_test(rng),
            state_root: Hash256::random_for_test(rng),
            body: BeaconBlockBodyDeneb::random_for_test(rng),
        };
        let block = BeaconBlock::Deneb(inner_block.clone());

        test_ssz_tree_hash_pair_with(&block, &inner_block, |bytes| {
            BeaconBlock::from_ssz_bytes(bytes, spec)
        });
    }

    #[test]
    fn decode_base_and_altair() {
        type E = MainnetEthSpec;
        let mut spec = E::default_spec();

        let rng = &mut XorShiftRng::from_seed([42; 16]);

        let altair_fork_epoch = spec.altair_fork_epoch.unwrap();

        let base_epoch = altair_fork_epoch.saturating_sub(1_u64);
        let base_slot = base_epoch.end_slot(E::slots_per_epoch());
        let altair_epoch = altair_fork_epoch;
        let altair_slot = altair_epoch.start_slot(E::slots_per_epoch());
        let capella_epoch = altair_fork_epoch + 1;
        let capella_slot = capella_epoch.start_slot(E::slots_per_epoch());
        let deneb_epoch = capella_epoch + 1;
        let deneb_slot = deneb_epoch.start_slot(E::slots_per_epoch());

        spec.altair_fork_epoch = Some(altair_epoch);
        spec.capella_fork_epoch = Some(capella_epoch);
        spec.deneb_fork_epoch = Some(deneb_epoch);

        // BeaconBlockBase
        {
            let good_base_block = BeaconBlock::Base(BeaconBlockBase {
                slot: base_slot,
                ..<_>::random_for_test(rng)
            });
            // It's invalid to have a base block with a slot higher than the fork epoch.
            let bad_base_block = {
                let mut bad = good_base_block.clone();
                *bad.slot_mut() = altair_slot;
                bad
            };

            assert_eq!(
                BeaconBlock::from_ssz_bytes(&good_base_block.as_ssz_bytes(), &spec)
                    .expect("good base block can be decoded"),
                good_base_block
            );
            BeaconBlock::from_ssz_bytes(&bad_base_block.as_ssz_bytes(), &spec)
                .expect_err("bad base block cannot be decoded");
        }

        // BeaconBlockAltair
        {
            let good_altair_block = BeaconBlock::Altair(BeaconBlockAltair {
                slot: altair_slot,
                ..<_>::random_for_test(rng)
            });
            // It's invalid to have an Altair block with a epoch lower than the fork epoch.
            let bad_altair_block = {
                let mut bad = good_altair_block.clone();
                *bad.slot_mut() = base_slot;
                bad
            };

            assert_eq!(
                BeaconBlock::from_ssz_bytes(&good_altair_block.as_ssz_bytes(), &spec)
                    .expect("good altair block can be decoded"),
                good_altair_block
            );
            BeaconBlock::from_ssz_bytes(&bad_altair_block.as_ssz_bytes(), &spec)
                .expect_err("bad altair block cannot be decoded");
        }

        // BeaconBlockCapella
        {
            let good_block = BeaconBlock::Capella(BeaconBlockCapella {
                slot: capella_slot,
                ..<_>::random_for_test(rng)
            });
            // It's invalid to have an Capella block with a epoch lower than the fork epoch.
            let bad_block = {
                let mut bad = good_block.clone();
                *bad.slot_mut() = altair_slot;
                bad
            };

            assert_eq!(
                BeaconBlock::from_ssz_bytes(&good_block.as_ssz_bytes(), &spec)
                    .expect("good capella block can be decoded"),
                good_block
            );
            BeaconBlock::from_ssz_bytes(&bad_block.as_ssz_bytes(), &spec)
                .expect_err("bad capella block cannot be decoded");
        }

        // BeaconBlockDeneb
        {
            let good_block = BeaconBlock::Deneb(BeaconBlockDeneb {
                slot: deneb_slot,
                ..<_>::random_for_test(rng)
            });
            // It's invalid to have an Capella block with a epoch lower than the fork epoch.
            let bad_block = {
                let mut bad = good_block.clone();
                *bad.slot_mut() = capella_slot;
                bad
            };

            assert_eq!(
                BeaconBlock::from_ssz_bytes(&good_block.as_ssz_bytes(), &spec)
                    .expect("good deneb block can be decoded"),
                good_block
            );
            BeaconBlock::from_ssz_bytes(&bad_block.as_ssz_bytes(), &spec)
                .expect_err("bad deneb block cannot be decoded");
        }
    }
}
