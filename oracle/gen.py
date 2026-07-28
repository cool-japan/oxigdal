import h5py, numpy as np, struct

def make(fname, data, chunks, maxshape=None):
    with h5py.File(fname,'w',libver='earliest') as f:
        f.create_dataset('d', data=data, chunks=chunks, maxshape=maxshape)

def dump(fname, ndims, label):
    b=open(fname,'rb').read()
    key_size=8+(ndims+1)*8; stride=key_size+8
    print(f"=== {label} ({fname}) ndims={ndims} ===")
    off=0
    while True:
        i=b.find(b'TREE',off)
        if i<0: break
        if b[i+4]==1:
            level=b[i+5]; nchild=struct.unpack_from('<H',b,i+6)[0]
            print(f" TREE@{i} level={level} nchild={nchild}")
            base=i+24
            for k in range(nchild+1):
                ko=base+k*stride
                nbytes,mask=struct.unpack_from('<II',b,ko)
                offs=struct.unpack_from(f'<{ndims+1}Q',b,ko+8)
                kind='TERMINAL' if k==nchild else f'key{k}'
                # only print first/last few keys for big nodes
                if nchild<=8 or k<2 or k>=nchild-1:
                    print(f"   {kind}: nbytes={nbytes} mask={mask} offs={list(offs)}")
        off=i+4

# 2D: dim0 single chunk, dim1 multiple  [3,7] chunk[3,3] -> grid 1x3
make('a_1x3.h5', np.arange(21,dtype='int32').reshape(3,7),(3,3)); dump('a_1x3.h5',2,'[3,7] chunk[3,3] grid1x3')
# 2D: dim0 multiple, dim1 single [7,3] chunk[3,3] -> grid 3x1
make('b_3x1.h5', np.arange(21,dtype='int32').reshape(7,3),(3,3)); dump('b_3x1.h5',2,'[7,3] chunk[3,3] grid3x1')
# 2D even grid 2x2 [4,4] chunk[2,2]
make('c_2x2.h5', np.arange(16,dtype='int32').reshape(4,4),(2,2)); dump('c_2x2.h5',2,'[4,4] chunk[2,2] grid2x2 even')
# 3D [4,3,2] chunk[2,2,2] -> grid 2x2x1
make('d_3d.h5', np.arange(24,dtype='int32').reshape(4,3,2),(2,2,2)); dump('d_3d.h5',3,'[4,3,2] chunk[2,2,2]')
# 3D single chunk [2,2,2] chunk[2,2,2]
make('e_3d1.h5', np.arange(8,dtype='int32').reshape(2,2,2),(2,2,2)); dump('e_3d1.h5',3,'[2,2,2] chunk[2,2,2] single')
# 1D 100 chunks -> multi-node tree (chunk 1)
make('f_100.h5', np.arange(100,dtype='int32'),(1,)); dump('f_100.h5',1,'[100] chunk[1] 100chunks multinode')
# unlimited dim0: shape[5] chunk[2] maxshape None
make('g_unlim.h5', np.arange(5,dtype='int32'),(2,),maxshape=(None,)); dump('g_unlim.h5',1,'[5] chunk[2] unlimited')
# oversized chunk (h5py rejects>data? try equal) - chunk == shape but 1D
make('h_1chunk.h5', np.arange(5,dtype='int32'),(5,)); dump('h_1chunk.h5',1,'[5] chunk[5] single')
