use std::{ptr, ffi::{c_int, c_void}};
use sundials_sys::*;

#[test]
fn cvode_create() {
    extern "C" fn f(
        _t: f64, _nvy: N_Vector, _nvdy: N_Vector, _user_data: *mut c_void,
    ) -> c_int {
        0
    }

    let lmm = CV_ADAMS;
    let mut ctx = ptr::null_mut();
    let comm = comm_no_mpi();
    unsafe {
        assert!(SUNContext_Create(comm, &mut ctx) >= 0);
        let mut cvode_mem = CVodeCreate(lmm, ctx);
        assert!(! cvode_mem.is_null());
        let mut y0: [realtype; 2] = [1., 2.];
        let y0: N_Vector = N_VMake_Serial(
            y0.len().try_into().unwrap(),
            y0.as_mut_ptr(),
            ctx);
        assert!(! y0.is_null());
        let r = CVodeInit(cvode_mem, Some(f), 0., y0);
        assert_eq!(r, CV_SUCCESS as i32);

        let linsolver = SUNLinSol_SPGMR(y0, SUN_PREC_NONE as _, 30, ctx);
        assert!(! linsolver.is_null());
	let r = CVodeSetLinearSolver(cvode_mem, linsolver, ptr::null_mut());
        assert_eq!(r, CVLS_SUCCESS as i32);

        SUNLinSolFree(linsolver);
        CVodeFree(&mut cvode_mem);
        SUNContext_Free(&mut ctx);
    }
}
