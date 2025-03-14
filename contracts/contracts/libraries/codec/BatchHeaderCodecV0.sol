// SPDX-License-Identifier: MIT

pragma solidity ^0.8.24;

// solhint-disable no-inline-assembly

/// @dev Below is the encoding for `BatchHeader` V0, total 249
/// ```text
///   * Field                   Bytes       Type        Index   Comments
///   * version                 1           uint8       0       The batch version
///   * batchIndex              8           uint64      1       The index of the batch
///   * l1MessagePopped         8           uint64      9       Number of L1 messages popped in the batch
///   * totalL1MessagePopped    8           uint64      17      Number of total L1 messages popped after the batch
///   * dataHash                32          bytes32     25      The data hash of the batch
///   * blobVersionedHash       32          bytes32     57      The versioned hash of the blob with this batch’s data
///   * prevStateHash           32          bytes32     89      Preview state root
///   * postStateHash           32          bytes32     121     Post state root
///   * withdrawRootHash        32          bytes32     153     L2 withdrawal tree root hash
///   * sequencerSetVerifyHash  32          bytes32     185     L2 sequencers set verify hash
///   * parentBatchHash         32          bytes32     217     The parent batch hash
/// ```
library BatchHeaderCodecV0 {
    /// @dev The length of fixed parts of the batch header.
    uint256 internal constant BATCH_HEADER_LENGTH = 249;

    /// @notice Load batch header in calldata to memory.
    /// @param _batchHeader The encoded batch header bytes in calldata.
    /// @return batchPtr The start memory offset of the batch header in memory.
    /// @return length The length in bytes of the batch header.
    function loadAndValidate(bytes calldata _batchHeader) internal pure returns (uint256 batchPtr, uint256 length) {
        length = _batchHeader.length;
        require(length >= BATCH_HEADER_LENGTH, "batch header length too small");
        // copy batch header to memory.
        assembly {
            batchPtr := mload(0x40)
            calldatacopy(batchPtr, _batchHeader.offset, length)
            mstore(0x40, add(batchPtr, length))
        }
    }

    /// @notice Get the version of the batch header.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @return _version The version of the batch header.
    function getVersion(uint256 batchPtr) internal pure returns (uint256 _version) {
        assembly {
            _version := shr(248, mload(batchPtr))
        }
    }

    /// @notice Get the batch index of the batch.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @return _batchIndex The batch index of the batch.
    function getBatchIndex(uint256 batchPtr) internal pure returns (uint256 _batchIndex) {
        assembly {
            _batchIndex := shr(192, mload(add(batchPtr, 1)))
        }
    }

    /// @notice Get the number of L1 messages of the batch.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @return _l1MessagePopped The number of L1 messages of the batch.
    function getL1MessagePopped(uint256 batchPtr) internal pure returns (uint256 _l1MessagePopped) {
        assembly {
            _l1MessagePopped := shr(192, mload(add(batchPtr, 9)))
        }
    }

    /// @notice Get the number of L1 messages popped before this batch.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @return _totalL1MessagePopped The the number of L1 messages popped before this batch.
    function getTotalL1MessagePopped(uint256 batchPtr) internal pure returns (uint256 _totalL1MessagePopped) {
        assembly {
            _totalL1MessagePopped := shr(192, mload(add(batchPtr, 17)))
        }
    }

    /// @notice Get the data hash of the batch header.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @return _dataHash The data hash of the batch header.
    function getDataHash(uint256 batchPtr) internal pure returns (bytes32 _dataHash) {
        assembly {
            _dataHash := mload(add(batchPtr, 25))
        }
    }

    /// @notice Get the blob versioned hash of the batch header.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @return _blobVersionedHash The blob versioned hash of the batch header.
    function getBlobVersionedHash(uint256 batchPtr) internal pure returns (bytes32 _blobVersionedHash) {
        assembly {
            _blobVersionedHash := mload(add(batchPtr, 57))
        }
    }

    function getPrevStateHash(uint256 batchPtr) internal pure returns (bytes32 _prevStateHash) {
        assembly {
            _prevStateHash := mload(add(batchPtr, 89))
        }
    }

    function getPostStateHash(uint256 batchPtr) internal pure returns (bytes32 _postStateHash) {
        assembly {
            _postStateHash := mload(add(batchPtr, 121))
        }
    }

    function getWithdrawRootHash(uint256 batchPtr) internal pure returns (bytes32 _withdrawRootHash) {
        assembly {
            _withdrawRootHash := mload(add(batchPtr, 153))
        }
    }

    function getSequencerSetVerifyHash(uint256 batchPtr) internal pure returns (bytes32 _sequencerSetVerifyHash) {
        assembly {
            _sequencerSetVerifyHash := mload(add(batchPtr, 185))
        }
    }

    /// @notice Get the parent batch hash of the batch header.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @return _parentBatchHash The parent batch hash of the batch header.
    function getParentBatchHash(uint256 batchPtr) internal pure returns (bytes32 _parentBatchHash) {
        assembly {
            _parentBatchHash := mload(add(batchPtr, 217))
        }
    }

    /// @notice Store the version of batch header.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @param _version The version of batch header.
    function storeVersion(uint256 batchPtr, uint256 _version) internal pure {
        assembly {
            mstore8(batchPtr, _version)
        }
    }

    /// @notice Store the batch index of batch header.
    /// @dev Because this function can overwrite the subsequent fields, it must be called before
    /// `storeL1MessagePopped`, `storeTotalL1MessagePopped`, and `storeDataHash`.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @param _batchIndex The batch index.
    function storeBatchIndex(uint256 batchPtr, uint256 _batchIndex) internal pure {
        assembly {
            mstore(add(batchPtr, 1), shl(192, _batchIndex))
        }
    }

    /// @notice Store the number of L1 messages popped in current batch to batch header.
    /// @dev Because this function can overwrite the subsequent fields, it must be called before
    /// `storeTotalL1MessagePopped` and `storeDataHash`.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @param _l1MessagePopped The number of L1 messages popped in current batch.
    function storeL1MessagePopped(uint256 batchPtr, uint256 _l1MessagePopped) internal pure {
        assembly {
            mstore(add(batchPtr, 9), shl(192, _l1MessagePopped))
        }
    }

    /// @notice Store the total number of L1 messages popped after current batch to batch header.
    /// @dev Because this function can overwrite the subsequent fields, it must be called before
    /// `storeDataHash`.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @param _totalL1MessagePopped The total number of L1 messages popped after current batch.
    function storeTotalL1MessagePopped(uint256 batchPtr, uint256 _totalL1MessagePopped) internal pure {
        assembly {
            mstore(add(batchPtr, 17), shl(192, _totalL1MessagePopped))
        }
    }

    /// @notice Store the data hash of batch header.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @param _dataHash The data hash.
    function storeDataHash(uint256 batchPtr, bytes32 _dataHash) internal pure {
        assembly {
            mstore(add(batchPtr, 25), _dataHash)
        }
    }

    /// @notice Store the parent batch hash of batch header.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @param _blobVersionedHash The versioned hash of the blob with this batch’s data.
    function storeBlobVersionedHash(uint256 batchPtr, bytes32 _blobVersionedHash) internal pure {
        assembly {
            mstore(add(batchPtr, 57), _blobVersionedHash)
        }
    }

    /// @dev Stores the previous state hash.
    /// @param batchPtr The memory pointer to the location where the previous state hash will be stored.
    /// @param _prevStateHash The hash of the previous state to be stored.
    function storePrevStateHash(uint256 batchPtr, bytes32 _prevStateHash) internal pure {
        assembly {
            mstore(add(batchPtr, 89), _prevStateHash)
        }
    }

    /// @dev Stores the post-state hash.
    /// @param batchPtr The memory pointer to the location where the post-state hash will be stored.
    /// @param _postStateHash The hash of the post-state to be stored.
    function storePostStateHash(uint256 batchPtr, bytes32 _postStateHash) internal pure {
        assembly {
            mstore(add(batchPtr, 121), _postStateHash)
        }
    }

    /// @dev Stores the withdrawal root hash.
    /// @param batchPtr The memory pointer to the location where the hash will be stored.
    /// @param _withdrawRootHash The hash of the withdrawal root to be stored.
    function storeWithdrawRootHash(uint256 batchPtr, bytes32 _withdrawRootHash) internal pure {
        assembly {
            mstore(add(batchPtr, 153), _withdrawRootHash)
        }
    }

    /// @dev Stores the hash for verifying the sequencer set.
    /// @param batchPtr The memory pointer to the batch data.
    /// @param _sequencerSetVerifyHash The hash of the sequencer set to be stored.
    function storeSequencerSetVerifyHash(uint256 batchPtr, bytes32 _sequencerSetVerifyHash) internal pure {
        assembly {
            mstore(add(batchPtr, 185), _sequencerSetVerifyHash)
        }
    }

    /// @notice Store the parent batch hash of batch header.
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @param _parentBatchHash The parent batch hash.
    function storeParentBatchHash(uint256 batchPtr, bytes32 _parentBatchHash) internal pure {
        assembly {
            mstore(add(batchPtr, 217), _parentBatchHash)
        }
    }

    /// @notice Compute the batch hash.
    /// @dev Caller should make sure that the encoded batch header is correct.
    ///
    /// @param batchPtr The start memory offset of the batch header in memory.
    /// @param length The length of the batch.
    /// @return _batchHash The hash of the corresponding batch.
    function computeBatchHash(uint256 batchPtr, uint256 length) internal pure returns (bytes32 _batchHash) {
        // in the current version, the hash is: keccak(BatchHeader without timestamp)
        assembly {
            _batchHash := keccak256(batchPtr, length)
        }
    }
}
