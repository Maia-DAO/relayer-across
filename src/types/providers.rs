use alloy::{
    network::Ethereum,
    providers::{
        fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
        Identity, PendingTransactionBuilder, RootProvider,
    },
    transports::TransportResult,
};

pub type ChainProvider = FillProvider<
    JoinFill<
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
    >,
    RootProvider,
    Ethereum,
>;

pub type SubmittedTxEnvelope = TransportResult<PendingTransactionBuilder<Ethereum>>;
