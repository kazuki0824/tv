package com.maleicacid.tvinput.tis

import org.junit.Test

class TisInputIdResolverWp10Test {
    @Test fun ownInputInfoRequiresMatchingServiceIdentityAndNonBlankId() {
        val packageName = "com.maleicacid.tvinput"
        val serviceName = "com.maleicacid.tvinput.tis.MaleicacidTvInputService"
        check(TisInputIdResolver.isOwnInputInfoForTest(
            infoId = "com.maleicacid.tvinput/.tis.MaleicacidTvInputService",
            servicePackageName = packageName,
            serviceName = serviceName,
            ownPackageName = packageName,
            ownClassName = serviceName,
        ))
        check(!TisInputIdResolver.isOwnInputInfoForTest(
            infoId = "",
            servicePackageName = packageName,
            serviceName = serviceName,
            ownPackageName = packageName,
            ownClassName = serviceName,
        ))
        check(!TisInputIdResolver.isOwnInputInfoForTest(
            infoId = "other.input",
            servicePackageName = "other.package",
            serviceName = serviceName,
            ownPackageName = packageName,
            ownClassName = serviceName,
        ))
        check(!TisInputIdResolver.isOwnInputInfoForTest(
            infoId = "other.input",
            servicePackageName = packageName,
            serviceName = "other.Service",
            ownPackageName = packageName,
            ownClassName = serviceName,
        ))
    }
}
